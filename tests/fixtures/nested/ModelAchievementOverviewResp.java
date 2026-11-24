@Data
public class ModelAchievementOverviewResp {
    private ModelSection model;
    private WarningSection warning;
    private JudgedSection judged;
    private ProductSection product;
    private BattleSection battle;
    private InstructionSection instruction;
    @Data
    public static class ModelSection { private Long total; private Long mom; }
    @Data
    public static class WarningSection { private Long total; private Long mom; }
    @Data
    public static class JudgedSection { private Long total; private Long mom; }
    @Data
    public static class ProductSection { private Long total; private Long mom; }
    @Data
    public static class BattleSection { private Long total; private Long mom; }
    @Data
    public static class InstructionSection { private Long total; private Long mom; }
}
