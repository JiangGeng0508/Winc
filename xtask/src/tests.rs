//! 跨平台命令入口的回归测试。

use super::*;

#[test]
fn test_command_reaches_its_argument_parser_on_every_platform() {
    // 故意不给 --base 的值，验证 test --all 已进入参数解析，不启动嵌套的 cargo test。
    // 若平台条件错误地排除了 test 入口，这里会打印用法并返回退出码 2。
    let args = ["test", "--all", "--base"].map(String::from);
    assert_eq!(dispatch(&args), ExitCode::FAILURE);
}
