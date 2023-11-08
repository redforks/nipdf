use nipdf_macro::{pdf_object, TryFromIntObject};

#[derive(TryFromIntObject, Default, PartialEq, Eq, Clone, Copy)]
enum Algorithm {
    #[default]
    Undocument = 0,
    AES = 1,
    AESV2 = 2,
    Unpublished = 3,
    DefinedInDoc = 4,
}

#[derive(TryFromIntObject, PartialEq, Eq, Clone, Copy)]
enum StandardHandlerRevion {
    V1 = 2,
    V2 = 3,
    V4 = 4,
}

#[pdf_object(())]
trait EncryptDictTrait {
    #[typ("Name")]
    fn filter(&self) -> &str;

    #[typ("Name")]
    fn sub_filter(&self) -> Option<&str>;

    #[or_default]
    #[key("V")]
    #[try_from]
    fn algorithm(&self) -> Algorithm;

    #[key("Length")]
    #[default(40)]
    fn key_length(&self) -> u32;

    #[key("R")]
    #[try_from]
    fn revison(&self) -> StandardHandlerRevion;

    /// 32-byte long string.
    #[key("O")]
    fn owner_password(&self) -> String;

    /// 32-byte long string.
    #[key("U")]
    fn user_password(&self) -> String;
}
