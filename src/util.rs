use std::borrow::Cow;

pub(crate) trait StrExt {
    fn replace_with<'a, F, S>(&'a self, pattern: &str, replacer: F) -> Cow<'a, str>
    where
        F: FnMut(usize, usize, &'a str) -> S,
        S: AsRef<str>;
}

impl StrExt for str {
    fn replace_with<'a, F, S>(&'a self, pattern: &str, mut replacer: F) -> Cow<'a, str>
    where
        F: FnMut(usize, usize, &'a str) -> S,
        S: AsRef<str>,
    {
        let mut result = String::new();
        let mut lastpos = 0;

        for (idx, (pos, substr)) in self.match_indices(pattern).enumerate() {
            result.push_str(&self[lastpos..pos]);
            lastpos = pos + substr.len();
            let replacement = replacer(idx, pos, substr);
            result.push_str(replacement.as_ref());
        }

        if lastpos == 0 {
            Cow::Borrowed(self)
        } else {
            result.push_str(&self[lastpos..]);
            Cow::Owned(result)
        }
    }
}
