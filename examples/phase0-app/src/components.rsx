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

#[cfg(test)]
mod tests {
    #[test]
    fn plain_rust_tests_still_run() {
        assert_eq!(1 + 1, 2);
    }
}
