use super::FileWithXRef;

#[derive(Debug, PartialEq)]
enum FieldQuery<'a> {
    SearchEverywhere(&'a str),
    NameOnly(&'a str),
    NameValueExact(&'a str, &'a str),
    NameAndContainsValue(&'a str, &'a str),
}

impl<'a> FieldQuery<'a> {
    fn parse(s: &'a str) -> Self {
        if let Some(s) = s.strip_prefix('/') {
            if let Some(pos) = s.find('=') {
                let (name, value) = s.split_at(pos);
                let value = &value[1..];
                if let Some(name) = name.strip_suffix('*') {
                    Self::NameAndContainsValue(name, value)
                } else {
                    Self::NameValueExact(name, value)
                }
            } else {
                Self::NameOnly(s)
            }
        } else {
            Self::SearchEverywhere(s)
        }
    }
}

/// Return false if no objects match the query.
pub fn query(doc: &FileWithXRef, q: Option<&String>, ignore_case: bool) -> bool {
    todo!()
}

#[cfg(test)]
mod tests;
