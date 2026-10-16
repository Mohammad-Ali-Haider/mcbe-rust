use super::*;
#[test]
fn utf8_editing_history_drafts_and_limits_are_safe() {
    let mut c = Chat::default();
    c.begin(false);
    c.text("aé中");
    c.key(K::ArrowLeft);
    c.key(K::Backspace);
    assert_eq!(c.input, "a中");
    c.text("b");
    assert_eq!(c.input, "ab中");
    c.key(K::Home);
    c.key(K::Delete);
    assert_eq!(c.input, "b中");
    c.key(K::End);
    c.text("\r\n");
    assert_eq!(c.submit().as_deref(), Some("b中"));
    c.begin(false);
    c.text("draft");
    c.key(K::ArrowUp);
    assert_eq!(c.input, "b中");
    c.key(K::ArrowDown);
    assert_eq!(c.input, "draft");
    for i in 0..80 {
        c.begin(false);
        c.text(&format!("message {i}"));
        c.submit();
    }
    assert_eq!(c.history.len(), HISTORY);
    c.begin(false);
    c.text(&"é".repeat(400));
    assert_eq!(c.input.len(), 512);
    assert!(c.input_window(12).chars().count() <= 12);
}
#[test]
fn messages_expire_only_when_closed_and_scrolling_stays_in_history() {
    let mut c = Chat::default();
    for i in 0..80 {
        c.push(Kind::System, format!("message {i}"));
    }
    assert_eq!(c.messages.len(), HISTORY);
    let later = Instant::now() + Duration::from_secs(11);
    c.tick(later);
    assert!(c.lines(30, 8, later).is_empty());
    c.begin(false);
    assert_eq!(c.lines(30, 8, later).len(), 8);
    c.scroll(100000);
    assert_eq!(c.lines(30, 8, later).len(), 8);
    c.scroll(-100000);
    assert!(c.lines(30, 8, later).last().unwrap().1.ends_with("79"));
}
