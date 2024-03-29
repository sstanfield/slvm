use crate::types::SlFrom;
use bridge_types::ErrorStrings;
use compile_state::state::SloshVm;
use slvm::value::ValueType;
use slvm::{from_i56, to_i56, VMError, VMResult, Value};

impl SlFrom<i32> for Value {
    fn sl_from(value: i32, _vm: &mut SloshVm) -> VMResult<Self> {
        Ok(to_i56(value as i64))
    }
}
impl SlFrom<u32> for Value {
    fn sl_from(value: u32, _vm: &mut SloshVm) -> VMResult<Self> {
        Ok(to_i56(value as i64))
    }
}

impl SlFrom<&Value> for i32 {
    fn sl_from(value: &Value, vm: &mut SloshVm) -> VMResult<i32> {
        match value {
            Value::Int(num) => {
                let num = from_i56(num);
                num.try_into().map_err(|_| {
                    VMError::new_conversion(
                        "Provided slosh value too small to fit desired type.".to_string(),
                    )
                })
            }
            _ => Err(VMError::new_conversion(
                ErrorStrings::fix_me_mismatched_type(ValueType::Int.into(), value.display_type(vm)),
            )),
        }
    }
}
