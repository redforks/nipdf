//! Contains the `CIDInit` ProcSet
use super::{ok, Key, RuntimeDictionary, RuntimeValue};
use crate::{sname, Name};

macro_rules! built_in_ops {
    ($($k:literal => $v:expr),* $(,)?) => {
        std::iter::Iterator::collect(std::iter::IntoIterator::into_iter([$((Key::Name(Name::from_static($k)), RuntimeValue::BuiltInOp($v)),)*]))
    };
}

/// Returns Dictionary contains CIDInit ProcSet
pub fn cid_init_dict<'a>() -> RuntimeDictionary<'a> {
    built_in_ops!(
        "begincmap" =>|_| ok(),
        "endcmap" => |_| ok(),
        "CMapName" => |m| {
            // todo: should push defined CMapName dict value
            m.push(sname("cmap-name-todo"));
            ok()
        },
        "begincodespacerange" => |m| {
            // pop a int from stack, the code space range entries.
            m.pop()?;
            ok()
        },
        "endcodespacerange" => |_| ok(),
        "defineresource" => |m| {
            m.pop()?    ;
            m.pop()?;
            ok()
        }
    )
}
