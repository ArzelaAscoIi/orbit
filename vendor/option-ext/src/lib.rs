//! `OptionExt::contains`, the one method dirs-sys uses.

pub trait OptionExt<T> {
    /// Whether this is `Some` holding a value equal to `x`.
    fn contains<U>(&self, x: &U) -> bool
    where
        U: PartialEq<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn contains<U>(&self, x: &U) -> bool
    where
        U: PartialEq<T>,
    {
        self.as_ref().is_some_and(|v| x == v)
    }
}

#[cfg(test)]
mod tests {
    use super::OptionExt;

    #[test]
    fn contains_matches_only_an_equal_some() {
        assert!(Some(3).contains(&3));
        assert!(!Some(3).contains(&4));
        assert!(!None::<i32>.contains(&3));
    }
}
