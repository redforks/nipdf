//! Contains the `CIDInit` ProcSet
use super::{ok, Key, RuntimeDictionary, RuntimeValue};
use crate as prescript;
use prescript_macro::name;

macro_rules! built_in_ops {
    ($($k:expr => $v:expr),* $(,)?) => {
        std::iter::Iterator::collect(std::iter::IntoIterator::into_iter([$((Key::Name($k), RuntimeValue::BuiltInOp($v)),)*]))
    };
}

/// Returns Dictionary contains CIDInit ProcSet
pub fn cid_init_dict<'a>() -> RuntimeDictionary<'a> {
    built_in_ops!(
        name!("begincmap") =>|_| ok(),
        name!("endcmap") => |_| ok(),
        name!("CMapName") => |m| {
            // todo: should push defined CMapName dict value
            m.push(name!("cmap-name-todo"));
            ok()
        },
        name!("begincodespacerange") => |m| {
            // pop a int from stack, the code space range entries.
            m.pop()?;
            ok()
        },
        name!("endcodespacerange") => |_| ok(),
        name!("defineresource") => |m| {
            m.pop()?    ;
            m.pop()?;
            ok()
        }
    )
}
