use outou::prelude::*;

#[derive(Debug, Clone, PartialEq)]
pub struct User {
    pub name: String,
}

#[component]
pub fn UserCard(user: User) -> Element {
    <div class="user-card">
        {user.name}
    </div>
}

/// A labeled form field. `type` is a JSX attribute name that happens to
/// be a Rust keyword (grammar §5); the component's own parameter must
/// therefore be written as the raw identifier `r#type` (issue #6 fix
/// list item 3, HIGH-3: Dioxus's raw string-key syntax for such a name
/// only exists for elements, not components, so a keyword prop is only
/// representable via `r#type`).
#[component]
pub fn Field(label: String, r#type: String) -> Element {
    <div class="field">
        <label for="id">{label}</label>
        <input id="id" type={r#type.clone()} />
    </div>
}

#[cfg(test)]
mod tests {
    #[test]
    fn plain_rust_tests_still_run() {
        assert_eq!(1 + 1, 2);
    }
}
