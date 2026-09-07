use super::*;

// Traditional Chinese texts

#[test]
fn traditional_fantizhongwenceshi() {
    assert_eq!(
        detect_chinese_type("繁體中文測試"),
        ChineseType::Traditional
    );
}

#[test]
fn traditional_language_detection() {
    assert_eq!(
        detect_chinese_type("看來還需要設定書寫語言偵測功能（）。"),
        ChineseType::Traditional
    );
}

#[test]
fn traditional_preset_problem() {
    assert_eq!(
        detect_chinese_type("我記得預設的當時也是遇到些爛問題，所以早先卸載了"),
        ChineseType::Traditional
    );
}

#[test]
fn traditional_termux() {
    assert_eq!(
        detect_chinese_type("Termux 跟 UserLAnd 裝 Python 套件也要用奇怪的方法 "),
        ChineseType::Traditional
    );
}

#[test]
fn traditional_ad_exam() {
    assert_eq!(
        detect_chinese_type("剛考完他的廣告投放檢定，其中一個選項就是縣市"),
        ChineseType::Traditional
    );
}

#[test]
fn traditional_personal_ad() {
    assert_eq!(
        detect_chinese_type("不 personal 但確實是廣告"),
        ChineseType::Traditional
    );
}

#[test]
fn traditional_scary() {
    assert_eq!(detect_chinese_type("這好可怕"), ChineseType::Traditional);
}

#[test]
fn traditional_phone() {
    assert_eq!(
        detect_chinese_type("你有手機了噢？"),
        ChineseType::Traditional
    );
}

#[test]
fn traditional_move_project() {
    assert_eq!(
        detect_chinese_type("額現在跟我其他專案放在一起，可以直接搬嗎還是要重新開？"),
        ChineseType::Traditional
    );
}

// Simplified Chinese texts

#[test]
fn simplified_jiantizhongwenceshi() {
    assert_eq!(detect_chinese_type("简体中文测试"), ChineseType::Simplified);
}

#[test]
fn simplified_gcc_raspberry_pi() {
    assert_eq!(
        detect_chinese_type("gcc也是32位的。。。这个树莓派要没救了"),
        ChineseType::Simplified
    );
}

#[test]
fn simplified_chatgpt_contradiction() {
    assert_eq!(
        detect_chinese_type("我在想一些歪门邪道的事情，但是chatgpt和自己矛盾了¿"),
        ChineseType::Simplified
    );
}

#[test]
fn simplified_strange_discovery() {
    assert_eq!(
        detect_chinese_type("救命我发现了很奇怪的事情"),
        ChineseType::Simplified
    );
}

#[test]
fn simplified_pip_tag() {
    assert_eq!(
        detect_chinese_type("发现我的pip支持的tag是armv7的"),
        ChineseType::Simplified
    );
}

#[test]
fn simplified_chip_arch() {
    assert_eq!(
        detect_chinese_type("但是我的芯片架构是aarch64"),
        ChineseType::Simplified
    );
}

#[test]
fn simplified_system_python() {
    assert_eq!(
        detect_chinese_type("原来是系统自带的python是32位的"),
        ChineseType::Simplified
    );
}

// Unknown / indeterminate texts

#[test]
fn unknown_single_char() {
    assert_eq!(detect_chinese_type("噢"), ChineseType::Unknown);
}

#[test]
fn unknown_pip_debug() {
    assert_eq!(
        detect_chinese_type("我使用 pip debug --verbose"),
        ChineseType::Unknown
    );
}

#[test]
fn unknown_english() {
    assert_eq!(
        detect_chinese_type("idk how to but maybe try"),
        ChineseType::Unknown
    );
}

#[test]
fn unknown_tensorflow() {
    assert_eq!(
        detect_chinese_type("用 tensorflow-cpu? "),
        ChineseType::Unknown
    );
}

#[test]
fn unknown_personal_ads() {
    assert_eq!(
        detect_chinese_type("就是 personal ads 吧"),
        ChineseType::Unknown
    );
}

#[test]
fn unknown_bushi() {
    assert_eq!(detect_chinese_type("不是"), ChineseType::Unknown);
}

#[test]
fn unknown_empty() {
    assert_eq!(detect_chinese_type(""), ChineseType::Unknown);
}

#[test]
fn unknown_whitespace() {
    assert_eq!(detect_chinese_type("   "), ChineseType::Unknown);
}
