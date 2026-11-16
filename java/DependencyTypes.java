import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.jar.JarFile;
import org.objectweb.asm.*;
import org.objectweb.asm.signature.SignatureReader;
import org.objectweb.asm.util.TraceSignatureVisitor;

/** 只读字节码；不加载业务类，避免静态初始化或 getter 带来副作用。 */
public final class DependencyTypes {
    public static void main(String[] args) {
        try {
            BufferedReader input = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
            String classpath = Objects.requireNonNull(input.readLine());
            List<Path> paths = Arrays.stream(classpath.split(java.util.regex.Pattern.quote(File.pathSeparator)))
                .filter(s -> !s.isBlank()).map(Path::of).toList();
            Map<String, String> result = new TreeMap<>();
            String name;
            int count = 0;
            while ((name = input.readLine()) != null) {
                if (++count > 128 || !name.matches("[A-Za-z_$][\\w$]*(\\.[A-Za-z_$][\\w$]*)*")) {
                    throw new IllegalArgumentException("invalid type request");
                }
                byte[] bytes = read(paths, name.replace('.', '/') + ".class");
                if (bytes != null) {
                    Source source = new Source();
                    new ClassReader(bytes).accept(source, ClassReader.SKIP_CODE | ClassReader.SKIP_DEBUG | ClassReader.SKIP_FRAMES);
                    result.put(name, source.text());
                }
            }
            StringJoiner json = new StringJoiner(",", "{", "}");
            result.forEach((key, value) -> json.add(quote(key) + ":" + quote(value)));
            System.out.print(json);
        } catch (Exception | LinkageError error) {
            // 路径、依赖日志或业务字符串不进入插件诊断。
            System.err.println("Java dependency metadata could not be read.");
            System.exit(1);
        }
    }

    private static byte[] read(List<Path> paths, String entry) throws IOException {
        for (Path path : paths) {
            if (Files.isDirectory(path)) {
                Path file = path.resolve(entry);
                if (Files.isRegularFile(file)) {
                    try (InputStream in = Files.newInputStream(file)) { return bounded(in); }
                }
            } else if (Files.isRegularFile(path)) {
                try (JarFile jar = new JarFile(path.toFile(), false, JarFile.OPEN_READ, Runtime.version())) {
                    var item = jar.getJarEntry(entry);
                    if (item != null) {
                        try (InputStream in = jar.getInputStream(item)) { return bounded(in); }
                    }
                }
            }
        }
        return null;
    }

    private static byte[] bounded(InputStream in) throws IOException {
        byte[] bytes = in.readNBytes(2 * 1024 * 1024 + 1);
        if (bytes.length > 2 * 1024 * 1024) throw new IOException("class too large");
        return bytes;
    }

    private static String type(String descriptor, String signature) {
        if (signature == null) return Type.getType(descriptor).getClassName();
        TraceSignatureVisitor visitor = new TraceSignatureVisitor(0);
        new SignatureReader(signature).acceptType(visitor);
        return visitor.getDeclaration();
    }

    private static String quote(String value) {
        StringBuilder out = new StringBuilder("\"");
        for (char c : value.toCharArray()) {
            if (c == '"' || c == '\\') out.append('\\').append(c);
            else if (c < 32) out.append(String.format("\\u%04x", (int)c));
            else out.append(c);
        }
        return out.append('"').toString();
    }

    private static AnnotationVisitor annotation(String descriptor, List<String> into) {
        String name = Type.getType(descriptor).getClassName();
        if (!(name.startsWith("com.fasterxml.jackson.annotation.") || name.startsWith("com.fasterxml.jackson.databind.annotation.")
            || name.startsWith("jakarta.validation.constraints.")
            || name.startsWith("javax.validation.constraints."))) return null;
        String shortName = name.substring(name.lastIndexOf('.') + 1);
        return new AnnotationVisitor(Opcodes.ASM9) {
            final List<String> values = new ArrayList<>();
            @Override public void visit(String key, Object value) {
                values.add(key + "=" + (value instanceof String ? quote((String)value) : String.valueOf(value)));
            }
            @Override public void visitEnum(String key, String desc, String value) {
                values.add(key + "=" + Type.getType(desc).getClassName().replace('$', '.') + "." + value);
            }
            @Override public AnnotationVisitor visitArray(String key) {
                // 未实现的 Jackson 数组语义保留为不支持标记，禁止悄悄丢失。
                values.add(key + "=UNSUPPORTED_ARRAY");
                return null;
            }
            @Override public void visitEnd() { into.add("@" + shortName + "(" + String.join(",", values) + ")"); }
        };
    }

    private static final class Source extends ClassVisitor {
        String packageName, className, declaration;
        final List<String> annotations = new ArrayList<>();
        final List<String> members = new ArrayList<>();
        Source() { super(Opcodes.ASM9); }
        @Override public void visit(int version, int access, String name, String signature, String parent, String[] interfaces) {
            int split = name.lastIndexOf('/');
            packageName = split < 0 ? "" : name.substring(0, split).replace('/', '.');
            className = name.substring(split + 1);
            String tail = "";
            if (signature != null) {
                TraceSignatureVisitor visitor = new TraceSignatureVisitor(access);
                new SignatureReader(signature).accept(visitor);
                tail = visitor.getDeclaration();
            } else if (parent != null && !parent.equals("java/lang/Object") && !parent.equals("java/lang/Record")) {
                tail = " extends " + parent.replace('/', '.');
            }
            String kind = (access & Opcodes.ACC_ENUM) != 0 ? "enum" : (access & Opcodes.ACC_INTERFACE) != 0 ? "interface" : "class";
            declaration = kind + " " + className + tail;
        }
        @Override public AnnotationVisitor visitAnnotation(String descriptor, boolean visible) {
            return annotation(descriptor, annotations);
        }
        @Override public FieldVisitor visitField(int access, String name, String descriptor, String signature, Object value) {
            if ((access & (Opcodes.ACC_STATIC | Opcodes.ACC_TRANSIENT | Opcodes.ACC_SYNTHETIC)) != 0) return null;
            List<String> anns = new ArrayList<>();
            return new FieldVisitor(Opcodes.ASM9) {
                @Override public AnnotationVisitor visitAnnotation(String descriptor, boolean visible) { return annotation(descriptor, anns); }
                @Override public void visitEnd() { members.add(String.join(" ", anns) + " " + type(descriptor, signature) + " " + name + ";"); }
            };
        }
        @Override public MethodVisitor visitMethod(int access, String name, String descriptor, String signature, String[] exceptions) {
            if ((access & (Opcodes.ACC_SYNTHETIC | Opcodes.ACC_BRIDGE)) != 0) return null;
            Type[] params = Type.getArgumentTypes(descriptor);
            boolean constructor = name.equals("<init>");
            boolean getter = (access & Opcodes.ACC_PUBLIC) != 0 && (access & Opcodes.ACC_STATIC) == 0
                && params.length == 0 && (name.startsWith("get") || name.startsWith("is"));
            List<String> anns = new ArrayList<>();
            List<List<String>> parameterAnnotations = new ArrayList<>();
            for (int i = 0; i < params.length; i++) parameterAnnotations.add(new ArrayList<>());
            return new MethodVisitor(Opcodes.ASM9) {
                @Override public AnnotationVisitor visitAnnotation(String descriptor, boolean visible) { return annotation(descriptor, anns); }
                @Override public AnnotationVisitor visitParameterAnnotation(int index, String descriptor, boolean visible) {
                    return annotation(descriptor, parameterAnnotations.get(index));
                }
                @Override public void visitEnd() {
                    if (constructor && anns.stream().noneMatch(a -> a.startsWith("@JsonCreator("))) return;
                    if (!constructor && !getter && anns.isEmpty()) return;
                    String returned = Type.getReturnType(descriptor).getClassName();
                    if (signature != null && !constructor) {
                        TraceSignatureVisitor visitor = new TraceSignatureVisitor(access);
                        new SignatureReader(signature).accept(visitor);
                        returned = visitor.getReturnType();
                    }
                    List<String> parameters = new ArrayList<>();
                    for (int i = 0; i < params.length; i++) parameters.add(String.join(" ", parameterAnnotations.get(i)) + " " + params[i].getClassName() + " p" + i);
                    members.add(String.join(" ", anns) + ((access & Opcodes.ACC_PUBLIC) != 0 ? " public " : " private ")
                        + ((access & Opcodes.ACC_STATIC) != 0 ? "static " : "") + (constructor ? className : returned + " " + name)
                        + "(" + String.join(",", parameters) + ") {}");
                }
            };
        }
        String text() {
            return (packageName.isEmpty() ? "" : "package " + packageName + ";\n") + String.join("\n", annotations)
                + "\n" + declaration + " {\n" + String.join("\n", members) + "\n}";
        }
    }
}
