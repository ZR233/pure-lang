pub(crate) fn preview_error(stderr: &str, stdout: &str) -> String {
    preview(format!("{stderr}\n{stdout}").trim(), 500)
}

fn preview(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &value[..end])
}
