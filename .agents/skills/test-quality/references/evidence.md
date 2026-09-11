# 测试质量方法论依据

本文保存 `test-quality` 的术语来源、工程经验和实证研究。仅在用户要求解释方法论、比较定义、提供书籍或论文依据，或质疑具体删测判断时读取；普通测试设计和审查直接使用 `SKILL.md`。

## 1. 术语与分类

测试层级没有唯一物理边界，因此技能把目标范围与执行条件分开记录，不以框架、目录、进程数或是否使用模拟对象单独命名。

- [ISTQB 基础级教学大纲 4.0.1](https://www.istqb.org/wp-content/uploads/2024/11/ISTQB_CTFL_Syllabus_v4.0.1.pdf) 区分组件测试、组件集成测试与系统集成测试。
- Martin Fowler 的 [单元测试](https://martinfowler.com/bliki/UnitTest.html) 区分孤立式与协作式单元测试；[集成测试](https://martinfowler.com/bliki/IntegrationTest.html) 说明该术语在不同团队中的范围并不一致。
- [《Software Engineering at Google》第 11 章](https://abseil.io/resources/swe-book/html/ch11.html) 把测试范围与测试规模分开：前者描述被验证的代码和行为，后者描述进程、网络、资源和时间成本。
- [Pact 官方文档](https://docs.pact.io/) 把契约测试描述为分别检查集成双方发送或接收的消息是否符合共同契约；它不要求两个完整系统同时部署，也不证明提供方的全部业务副作用。

## 2. 工程经验

下列来源共同支持按行为、可维护性、确定性和成本判断测试，但不能机械转化为固定比例。

- [《Software Engineering at Google》第 12 章](https://abseil.io/resources/swe-book/html/ch12.html) 建议通过公共接口测试行为，而不是逐个测试方法，并把随无关重构破裂的测试视为脆弱测试。
- [第 13 章](https://abseil.io/resources/swe-book/html/ch13.html) 说明真实实现提高保真度，测试替身提高隔离性；过度使用模拟对象容易与实现耦合并和真实依赖漂移。
- [第 14 章](https://abseil.io/resources/swe-book/html/ch14.html) 从隔离性、保真度、维护与运行成本讨论大型测试，不把系统测试当作单元测试的替代品。
- Gerard Meszaros 的 [《xUnit Test Patterns》](https://www.informit.com/store/xunit-test-patterns-refactoring-test-code-9780321504807) 把易运行、易理解、降低风险和低维护成本列为测试自动化目标。
- Martin Fowler 的 [《Mocks Aren't Stubs》](https://martinfowler.com/articles/mocksArentStubs.html) 指出交互测试容易耦合实现，同时承认模拟对象在复杂夹具和不可直接观察的交互契约中有价值。
- [《The Practical Test Pyramid》](https://martinfowler.com/articles/practical-test-pyramid.html) 建议将能以同等真实性在低层证明的场景下移，并删除不再提供价值的高成本重复测试。
- [Google Testing Blog 的测试金字塔文章](https://testing.googleblog.com/2015/04/just-say-no-to-more-end-to-end-tests.html) 把固定比例明确描述为起始猜测，而非通用标准。
- [Jest 快照测试文档](https://jestjs.io/docs/30.0/snapshot-testing) 要求快照短小、确定、可审查，更新前确认差异属于预期行为变化。
- [理解覆盖率数据](https://testing.googleblog.com/2008/03/tott-understanding-your-coverage-data.html) 指出高覆盖率不是良好测试的充分条件；执行代码不等于正确验证代码。

## 3. 实证研究

这些研究支持缺陷敏感度、变异测试、flaky、模拟对象和测试代码缺陷方面的判断。多数样本来自 Java、开源仓库或单一公司，使用时必须说明外部有效性限制。

- [测试用例质量的信念与证据研究](https://arxiv.org/abs/2307.06410) 在 42 个成熟 Java 项目中只找到对若干常见静态质量假设的极弱支持，说明静态特征不足以单独预测缺陷发现能力。
- [变异测试的长期影响](https://research.google/pubs/long-term-effects-of-mutation-testing/) 分析 Google 多年变异测试数据，观察到开发者增加测试且后续存活变异体减少；这不能推出变异分数可以替代真实风险模型。
- [Flaky 测试实证分析](https://experts.illinois.edu/en/publications/an-empirical-analysis-of-flaky-tests/) 在 Apache 项目样本中发现异步等待、并发和顺序依赖是主要根因之一；样本只覆盖已修复且可检索案例。
- [测试代码缺陷实证研究](https://people.ece.ubc.ca/amesbah/resources/papers/icsme15.pdf) 说明测试自身错误既会制造误报，也会在产品错误时保持绿色，后者常与错误或缺失断言有关；样本不代表所有测试错误。
- [Java 系统中的模拟对象研究](https://link.springer.com/article/10.1007/s10664-018-9663-0) 说明 mock 有助于隔离昂贵依赖，但会降低现实性，并随生产接口或内部实现变化产生维护成本。

## 4. 项目选择与解释边界

项目以最少且充分的测试证明完整通用功能。以下区分外部资料支持的方法和项目自行确定的约束，避免把工程选择写成书籍的普遍结论。

### 4.1 行为与组织

[Software Engineering at Google，第 12 章](https://abseil.io/resources/swe-book/html/ch12.html) 的 “Test Behaviors, Not Methods” 和 “Test via Public APIs” 支持按行为组织、减少实现耦合；书中仍使用具体输入，也保留无效交易不改变余额的验证，并未主张删除所有错误输入测试。[The Practical Test Pyramid](https://martinfowler.com/articles/practical-test-pyramid.html) 支持删除没有额外价值的跨层重复证明，不要求把所有测试都变成端到端测试。

[The Rust Programming Language：Test Organization](https://doc.rust-lang.org/book/ch11-03-test-organization.html) 说明源码内 `#[cfg(test)]` 单元测试可访问私有接口，`tests/` 集成测试作为外部消费者使用公开接口。书中描述语言机制与常见组织方式，不意味着每个私有函数都需要测试。

### 4.2 项目约束

单元测试固定放在对应源码末尾、禁止测试通过 `#[path]` 或其他源码包含方式引入生产实现，是本项目为保持正常编译与接口边界选择的约束。删除独立参数回读和配置清单镜像，将有实际后果的输入拒绝并入完整功能验证，也是项目组织规则；这些要求不应归因于书籍禁止固定数值或所有边界测试。

项目要求错误修复具备确定性红绿证明，但优先复用或增强已有通用功能测试，不要求每个历史问题新增用例。去重不能丢弃独特失败信号，也不把零测试拒绝、终态判定或必要平台执行当作冗余参数检查撤销。

上述来源不能推出固定测试比例、覆盖率阈值、变异分数或“永远不用模拟对象”的规则，也不能证明有限样例覆盖所有输入。本项目不强制使用随机生成或属性测试框架；选择样例和测试层级仍取决于功能规则、独特错误信号及运行成本。
