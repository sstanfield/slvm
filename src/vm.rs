use crate::chunk::*;
use crate::error::*;
use crate::heap::*;
use crate::interner::*;
use crate::opcodes::*;
use crate::value::*;

macro_rules! decode_u16 {
    ($vm:expr) => {{
        let idx1 = $vm.chunk.code[$vm.ip];
        let idx2 = $vm.chunk.code[$vm.ip + 1];
        $vm.ip += 2;
        ((idx1 as u16) << 8) | (idx2 as u16)
    }};
}

macro_rules! decode2 {
    ($vm:expr, $wide:expr) => {{
        let op1 = if $wide {
            decode_u16!($vm)
        } else {
            let ret = $vm.chunk.code[$vm.ip] as u16;
            $vm.ip += 1;
            ret
        };
        let op2 = if $wide {
            decode_u16!($vm)
        } else {
            let ret = $vm.chunk.code[$vm.ip] as u16;
            $vm.ip += 1;
            ret
        };
        (op1, op2)
    }};
}

macro_rules! decode3 {
    ($vm:expr, $wide:expr) => {{
        let op1 = if $wide {
            decode_u16!($vm)
        } else {
            let ret = $vm.chunk.code[$vm.ip] as u16;
            $vm.ip += 1;
            ret
        };
        let op2 = if $wide {
            decode_u16!($vm)
        } else {
            let ret = $vm.chunk.code[$vm.ip] as u16;
            $vm.ip += 1;
            ret
        };
        let op3 = if $wide {
            decode_u16!($vm)
        } else {
            let ret = $vm.chunk.code[$vm.ip] as u16;
            $vm.ip += 1;
            ret
        };
        (op1, op2, op3)
    }};
}

macro_rules! binary_math {
    ($vm:expr, $registers:expr, $bin_fn:expr, $wide:expr, $op2_reg:expr, $op3_reg:expr) => {{
        let (dest, op2, op3) = decode3!($vm, $wide);
        let op2 = if $op2_reg {
            $registers[op2 as usize]
        } else {
            $vm.chunk.constants[op2 as usize]
        };
        let op3 = if $op3_reg {
            $registers[op3 as usize]
        } else {
            $vm.chunk.constants[op3 as usize]
        };
        let val = if op2.is_int() && op3.is_int() {
            Value::Int($bin_fn(op2.get_int()?, op3.get_int()?))
        } else {
            Value::Float($bin_fn(op2.get_float()?, op3.get_float()?))
        };
        set_register!($registers, dest, val);
    }};
}

macro_rules! div_math {
    ($vm:expr, $registers:expr, $wide:expr, $op2_reg:expr, $op3_reg:expr) => {{
        let (dest, op2, op3) = decode3!($vm, $wide);
        let op2 = if $op2_reg {
            $registers[op2 as usize]
        } else {
            $vm.chunk.constants[op2 as usize]
        };
        let op3 = if $op3_reg {
            $registers[op3 as usize]
        } else {
            $vm.chunk.constants[op3 as usize]
        };
        let val = if op2.is_int() && op3.is_int() {
            let op3 = op3.get_int()?;
            if op3 == 0 {
                return Err(VMError::new_vm("Divide by zero error."));
            }
            Value::Int(op2.get_int()? / op3)
        } else {
            let op3 = op3.get_float()?;
            if op3 == 0.0 {
                return Err(VMError::new_vm("Divide by zero error."));
            }
            Value::Float(op2.get_float()? / op3)
        };
        set_register!($registers, dest, val);
        // Ok(())
    }};
}

macro_rules! set_register {
    ($registers:expr, $idx:expr, $val:expr) => {{
        $registers[$idx as usize] = $val;
        /*unsafe {
            let r = $registers.get_unchecked_mut($idx as usize);
            *r = $val;
        }*/
    }};
}

pub struct Vm {
    interner: Interner,
    heap: Heap,
    chunk: Chunk,
    stack: Vec<Value>,
    ip: usize,
    globals: Globals,
}

impl Vm {
    pub fn new(chunk: Chunk) -> Self {
        //let root = interner.intern("root");
        let globals = Globals::new();
        let mut stack = Vec::with_capacity(1024);
        stack.resize(1024, Value::Undefined);
        Vm {
            interner: Interner::with_capacity(8192),
            heap: Heap::new(),
            chunk,
            stack,
            ip: 0,
            globals,
        }
    }

    pub fn alloc(&mut self, obj: Object) -> Handle {
        self.heap.alloc(obj, |_heap| Ok(()))
    }

    pub fn get(&self, handle: Handle) -> VMResult<HandleRef<'_>> {
        Ok(self.heap.get(handle)?)
    }

    pub fn intern(&mut self, string: &str) -> Interned {
        self.interner.intern(string)
    }

    pub fn intern_symbol_empty(&mut self, string: &str) -> Interned {
        let handle = self.alloc(Object::Value(Value::Undefined));
        self.globals.intern_symbol(string, handle)
    }

    pub fn intern_symbol(&mut self, string: &str, handle: Handle) -> Interned {
        self.globals.intern_symbol(string, handle)
    }

    fn list(&mut self, registers: &mut [Value], wide: bool) -> VMResult<()> {
        let (dest, start, end) = decode3!(self, wide);
        if end == start {
            set_register!(registers, dest, Value::Nil);
        } else {
            let mut last_cdr = Value::Nil;
            for i in (start..end).rev() {
                let car = if let Some(op) = registers.get(i as usize) {
                    op
                } else {
                    return Err(VMError::new_vm("List: Not enough elements."));
                };
                let cdr = last_cdr;
                last_cdr = Value::Reference(self.alloc(Object::Pair(*car, cdr)));
            }
            set_register!(registers, dest, last_cdr);
        }
        Ok(())
    }

    fn xar(&mut self, registers: &mut [Value], wide: bool) -> VMResult<()> {
        let (pair_reg, val) = decode2!(self, wide);
        let pair = registers[pair_reg as usize];
        let val = registers[val as usize];
        match &pair {
            Value::Reference(cons_handle) => {
                let cons_d = self.heap.get(*cons_handle)?;
                if let Object::Pair(_car, cdr) = &*cons_d {
                    let cdr = *cdr;
                    self.heap.replace(*cons_handle, Object::Pair(val, cdr))?;
                } else if cons_d.is_nil() {
                    let pair = Object::Pair(val, Value::Nil);
                    self.heap.replace(*cons_handle, pair)?;
                } else {
                    return Err(VMError::new_vm("XAR: Not a pair/conscell."));
                }
            }
            Value::Nil => {
                let pair = Value::Reference(self.alloc(Object::Pair(val, Value::Nil)));
                set_register!(registers, pair_reg, pair);
            }
            _ => {
                return Err(VMError::new_vm("XAR: Not a pair/conscell."));
            }
        }
        Ok(())
    }

    fn xdr(&mut self, registers: &mut [Value], wide: bool) -> VMResult<()> {
        let (pair_reg, val) = decode2!(self, wide);
        let pair = registers[pair_reg as usize];
        let val = registers[val as usize];
        match &pair {
            Value::Reference(cons_handle) => {
                let cons_d = self.heap.get(*cons_handle)?;
                if let Object::Pair(car, _cdr) = &*cons_d {
                    let car = *car;
                    self.heap.replace(*cons_handle, Object::Pair(car, val))?;
                } else if cons_d.is_nil() {
                    let pair = Object::Pair(Value::Nil, val);
                    self.heap.replace(*cons_handle, pair)?;
                } else {
                    return Err(VMError::new_vm("XAR: Not a pair/conscell."));
                }
            }
            Value::Nil => {
                let pair = Value::Reference(self.alloc(Object::Pair(Value::Nil, val)));
                set_register!(registers, pair_reg, pair);
            }
            _ => {
                return Err(VMError::new_vm("XAR: Not a pair/conscell."));
            }
        }
        Ok(())
    }

    // Need to break the registers lifetime away from self or we can not do much...
    // The underlying stack should never be deleted or reallocated for the life
    // of Vm so this should be safe.
    fn make_registers(&mut self, start: usize) -> &'static mut [Value] {
        unsafe { &mut *(&mut self.stack[start..] as *mut [Value]) }
    }

    pub fn execute(&mut self) -> VMResult<()> {
        let registers = self.make_registers(0);
        loop {
            let opcode = self.chunk.code[self.ip];
            self.ip += 1;
            let wide = (opcode & 0x80) != 0;
            match opcode & 0x7F {
                RET => {
                    return Ok(());
                }
                STORE => {
                    let (dest, src) = decode2!(self, wide);
                    let val = registers[src as usize];
                    set_register!(registers, dest, val);
                }
                STORE_K => {
                    let (dest, src) = decode2!(self, wide);
                    let val = self.chunk.constants[src as usize];
                    set_register!(registers, dest, val);
                }
                ADD => binary_math!(self, registers, |a, b| a + b, wide, true, true),
                ADD_RK => binary_math!(self, registers, |a, b| a + b, wide, true, false),
                ADD_KR => binary_math!(self, registers, |a, b| a + b, wide, false, true),
                SUB => binary_math!(self, registers, |a, b| a - b, wide, true, true),
                SUB_RK => binary_math!(self, registers, |a, b| a - b, wide, true, false),
                SUB_KR => binary_math!(self, registers, |a, b| a - b, wide, false, true),
                MUL => binary_math!(self, registers, |a, b| a * b, wide, true, true),
                MUL_RK => binary_math!(self, registers, |a, b| a * b, wide, true, false),
                MUL_KR => binary_math!(self, registers, |a, b| a * b, wide, false, true),
                DIV => div_math!(self, registers, wide, true, true),
                DIV_RK => div_math!(self, registers, wide, true, false),
                DIV_KR => div_math!(self, registers, wide, false, true),
                CONS => {
                    let (dest, op2, op3) = decode3!(self, wide);
                    let car = registers[op2 as usize];
                    let cdr = registers[op3 as usize];
                    set_register!(
                        registers,
                        dest,
                        Value::Reference(self.alloc(Object::Pair(car, cdr)))
                    );
                }
                CAR => {
                    let (dest, op) = decode2!(self, wide);
                    let op = registers[op as usize];
                    match op.unref(self)? {
                        Value::Reference(handle) => {
                            let handle_d = self.heap.get(handle)?;
                            if let Object::Pair(car, _) = &*handle_d {
                                set_register!(registers, dest, *car);
                            } else {
                                return Err(VMError::new_vm("CAR: Not a pair/conscell."));
                            }
                        }
                        Value::Nil => set_register!(registers, dest, Value::Nil),
                        _ => return Err(VMError::new_vm("CAR: Not a pair/conscell.")),
                    }
                }
                CDR => {
                    let (dest, op) = decode2!(self, wide);
                    let op = registers[op as usize];
                    match op.unref(self)? {
                        Value::Reference(handle) => {
                            let handle_d = self.heap.get(handle)?;
                            if let Object::Pair(_, cdr) = &*handle_d {
                                set_register!(registers, dest, *cdr);
                            } else {
                                return Err(VMError::new_vm("CDR: Not a pair/conscell."));
                            }
                        }
                        Value::Nil => set_register!(registers, dest, Value::Nil),
                        _ => return Err(VMError::new_vm("CDR: Not a pair/conscell.")),
                    }
                }
                LIST => self.list(registers, wide)?,
                XAR => self.xar(registers, wide)?,
                XDR => self.xdr(registers, wide)?,
                _ => {
                    return Err(VMError::new_vm(format!("Invalid opcode {}", opcode)));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_int(_vm: &Vm, val: &Value) -> VMResult<i64> {
        if let Value::Int(i) = val {
            Ok(*i)
        } else {
            Err(VMError::new_vm("Not an int"))
        }
    }

    fn is_nil(_vm: &Vm, val: &Value) -> VMResult<bool> {
        if let Value::Nil = val {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    #[test]
    fn test_list() -> VMResult<()> {
        let mut chunk = Chunk::new("no_file", 1);
        let line = 1;
        chunk.add_constant(Value::Int(1));
        chunk.add_constant(Value::Int(2));
        chunk.add_constant(Value::Int(3));
        chunk.add_constant(Value::Int(4));
        chunk.add_constant(Value::Nil);
        chunk.encode2(STORE_K, 0, 0, line).unwrap();
        chunk.encode2(STORE_K, 1, 1, line).unwrap();
        chunk.encode3(CONS, 1, 0, 1, line).unwrap();
        chunk.encode2(CDR, 0, 1, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        let const_handle = vm.alloc(Object::Value(Value::Nil));
        vm.chunk.add_constant(Value::Reference(const_handle));
        vm.execute()?;
        let result = vm.stack[0].get_int()?;
        assert!(result == 2);

        vm.chunk.encode2(CAR, 0, 1, line).unwrap();
        vm.chunk.encode0(RET, line)?;
        vm.execute()?;
        let result = vm.stack[0].get_int()?;
        assert!(result == 1);

        // car with nil
        vm.chunk.encode2(STORE_K, 2, 4, line).unwrap();
        vm.chunk.encode2(CAR, 0, 2, line).unwrap();
        vm.chunk.encode0(RET, line)?;
        vm.execute()?;
        assert!(vm.stack[0].is_nil());
        // car with nil on heap
        vm.chunk.encode2(STORE_K, 2, 5, line).unwrap();
        vm.chunk.encode2(CAR, 0, 2, line).unwrap();
        vm.chunk.encode0(RET, line)?;
        vm.execute()?;
        assert!(vm.stack[0].is_nil());

        // cdr with nil
        vm.chunk.encode2(CDR, 0, 2, line).unwrap();
        vm.chunk.encode0(RET, line)?;
        vm.execute()?;
        assert!(vm.stack[0].is_nil());
        // cdr with nil on heap
        vm.chunk.encode2(STORE_K, 2, 5, line).unwrap();
        vm.chunk.encode2(CDR, 0, 2, line).unwrap();
        vm.chunk.encode0(RET, line)?;
        vm.execute()?;
        assert!(vm.stack[0].is_nil());

        vm.chunk.encode2(STORE_K, 2, 2, line).unwrap();
        vm.chunk.encode2(XAR, 1, 2, line).unwrap();
        vm.chunk.encode2(CAR, 0, 1, line).unwrap();
        vm.chunk.encode0(RET, line)?;
        vm.execute()?;
        let result = vm.stack[0].get_int()?;
        assert!(result == 3);

        vm.chunk.encode2(STORE_K, 2, 3, line).unwrap();
        vm.chunk.encode2(XDR, 1, 2, line).unwrap();
        vm.chunk.encode2(CDR, 0, 1, line).unwrap();
        vm.chunk.encode0(RET, line)?;
        vm.execute()?;
        let result = vm.stack[0].get_int()?;
        assert!(result == 4);

        vm.chunk.encode2(STORE_K, 2, 4, line).unwrap();
        vm.chunk.encode2(STORE_K, 3, 2, line).unwrap();
        vm.chunk.encode2(XAR, 2, 3, line).unwrap();
        vm.chunk.encode2(CAR, 0, 2, line).unwrap();
        vm.chunk.encode2(CDR, 3, 2, line).unwrap();
        vm.chunk.encode0(RET, line)?;
        vm.execute()?;
        let result = vm.stack[0].get_int()?;
        assert!(result == 3);
        assert!(vm.stack[3].is_nil());

        vm.chunk.encode2(STORE_K, 2, 4, line).unwrap();
        vm.chunk.encode2(STORE_K, 3, 3, line).unwrap();
        vm.chunk.encode2(XDR, 2, 3, line).unwrap();
        vm.chunk.encode2(CDR, 0, 2, line).unwrap();
        vm.chunk.encode2(CAR, 3, 2, line).unwrap();
        vm.chunk.encode0(RET, line)?;
        vm.execute()?;
        let result = vm.stack[0].get_int()?;
        assert!(result == 4);
        assert!(vm.stack[3].is_nil());

        // Test a list with elements.
        vm.chunk.encode2(STORE_K, 0, 0, line).unwrap();
        vm.chunk.encode2(STORE_K, 1, 1, line).unwrap();
        vm.chunk.encode2(STORE_K, 2, 2, line).unwrap();
        vm.chunk.encode3(LIST, 0, 0, 3, line).unwrap();
        vm.chunk.encode0(RET, line)?;
        vm.execute()?;
        let result = vm.stack.get(0).unwrap();
        if let Value::Reference(h) = result {
            if let Object::Pair(car, cdr) = &*vm.heap.get(*h)? {
                assert!(get_int(&vm, car)? == 1);
                if let Value::Reference(cdr) = cdr {
                    if let Object::Pair(car, cdr) = &*vm.heap.get(*cdr)? {
                        assert!(get_int(&vm, car)? == 2);
                        if let Value::Reference(cdr) = cdr {
                            if let Object::Pair(car, cdr) = &*vm.heap.get(*cdr)? {
                                assert!(get_int(&vm, car)? == 3);
                                assert!(is_nil(&vm, cdr)?);
                            } else {
                                assert!(false);
                            }
                        } else {
                            assert!(false);
                        }
                    } else {
                        assert!(false);
                    }
                } else {
                    assert!(false);
                }
            } else {
                assert!(false);
            }
        } else {
            assert!(false);
        }

        vm.chunk.encode3(LIST, 0, 0, 0, line).unwrap();
        vm.chunk.encode0(RET, line)?;
        vm.execute()?;
        let result = vm.stack.get(0).unwrap();
        assert!(result.is_nil());
        vm.chunk.encode3(LIST, 0, 1, 1, line).unwrap();
        vm.chunk.encode0(RET, line)?;
        vm.execute()?;
        let result = vm.stack.get(0).unwrap();
        assert!(result.is_nil());
        Ok(())
    }

    #[test]
    fn test_store() -> VMResult<()> {
        let mut chunk = Chunk::new("no_file", 1);
        let line = 1;
        for i in 0..u16::MAX {
            chunk.add_constant(Value::Int(i as i64));
        }
        chunk.encode2(STORE_K, 0, 0, line).unwrap();
        chunk.encode2(STORE_K, 1, 255, line).unwrap();
        chunk.encode3(ADD, 0, 0, 1, line).unwrap();
        chunk.encode0(RET, line)?;

        let mut vm = Vm::new(chunk);
        vm.execute()?;
        let result = vm.stack[0].get_int()?;
        assert!(result == 255);

        vm.chunk.encode2(STORE_K, 1, 256, line).unwrap();
        vm.chunk.encode3(ADD, 0, 0, 1, line).unwrap();
        vm.chunk.encode0(RET, line)?;

        vm.execute()?;
        let result = vm.stack[0].get_int()?;
        assert!(result == 255 + 256);

        vm.chunk.encode2(STORE, 1, 0, line).unwrap();
        vm.chunk.encode0(RET, line)?;
        let result = vm.stack[1].get_int()?;
        assert!(result == 256);
        vm.execute()?;
        let result = vm.stack[1].get_int()?;
        assert!(result == 255 + 256);

        Ok(())
    }

    #[test]
    fn test_add() -> VMResult<()> {
        let mut chunk = Chunk::new("no_file", 1);
        let line = 1;
        let const0 = chunk.add_constant(Value::Int(2 as i64)) as u16;
        let const1 = chunk.add_constant(Value::Int(3 as i64)) as u16;
        let const2 = chunk.add_constant(Value::Byte(1)) as u16;
        chunk.encode2(STORE_K, 0, const0, line)?;
        chunk.encode3(ADD_RK, 0, 0, const1, line).unwrap();
        chunk.encode3(ADD_RK, 0, 0, const2, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        vm.execute()?;
        assert!(vm.stack[0].get_int()? == 6);

        let mut chunk = Chunk::new("no_file", 1);
        let const0 = chunk.add_constant(Value::Float(2 as f64)) as u16;
        let const1 = chunk.add_constant(Value::Int(3 as i64)) as u16;
        let const2 = chunk.add_constant(Value::Byte(1)) as u16;
        chunk.encode2(STORE_K, 0, const0, line)?;
        chunk.encode3(ADD_RK, 0, 0, const1, line).unwrap();
        chunk.encode3(ADD_RK, 0, 0, const2, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        vm.execute()?;
        let item = vm.stack[0];
        assert!(!item.is_int());
        assert!(item.is_number());
        assert!(item.get_float()? == 6.0);

        let mut chunk = Chunk::new("no_file", 1);
        for i in 0..u16::MAX {
            chunk.add_constant(Value::Int(i as i64));
        }
        chunk.encode2(STORE_K, 1, 1, line)?;
        chunk.encode2(STORE_K, 2, 2, line)?;
        chunk.encode3(ADD, 0, 1, 2, line).unwrap();
        chunk.encode3(ADD_KR, 0, 5, 0, line).unwrap();
        chunk.encode3(ADD_KR, 1, 500, 0, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        vm.execute()?;
        let item = vm.stack[0];
        let item2 = vm.stack[1];
        assert!(item.is_int());
        assert!(item.get_int()? == 8);
        assert!(item2.get_int()? == 508);
        Ok(())
    }

    #[test]
    fn test_sub() -> VMResult<()> {
        let mut chunk = Chunk::new("no_file", 1);
        let line = 1;
        let const0 = chunk.add_constant(Value::Int(2 as i64)) as u16;
        let const1 = chunk.add_constant(Value::Int(3 as i64)) as u16;
        let const2 = chunk.add_constant(Value::Byte(1)) as u16;
        chunk.encode2(STORE_K, 0, const0, line)?;
        chunk.encode3(SUB_RK, 0, 0, const1, line).unwrap();
        chunk.encode3(SUB_RK, 0, 0, const2, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        vm.execute()?;
        assert!(vm.stack[0].get_int()? == -2);

        let mut chunk = Chunk::new("no_file", 1);
        let const0 = chunk.add_constant(Value::Float(5 as f64)) as u16;
        let const1 = chunk.add_constant(Value::Int(3 as i64)) as u16;
        let const2 = chunk.add_constant(Value::Byte(1)) as u16;
        chunk.encode2(STORE_K, 0, const0, line)?;
        chunk.encode3(SUB_RK, 0, 0, const1, line).unwrap();
        chunk.encode3(SUB_RK, 0, 0, const2, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        vm.execute()?;
        let item = vm.stack[0];
        assert!(!item.is_int());
        assert!(item.is_number());
        assert!(item.get_float()? == 1.0);

        let mut chunk = Chunk::new("no_file", 1);
        for i in 0..u16::MAX {
            chunk.add_constant(Value::Int(i as i64));
        }
        chunk.encode2(STORE_K, 1, 1, line)?;
        chunk.encode2(STORE_K, 2, 2, line)?;
        chunk.encode3(SUB, 0, 1, 2, line).unwrap();
        chunk.encode3(SUB_KR, 0, 5, 0, line).unwrap();
        chunk.encode3(SUB_KR, 1, 500, 0, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        vm.execute()?;
        let item = vm.stack[0];
        let item2 = vm.stack[1];
        assert!(item.is_int());
        assert!(item.get_int()? == 6);
        assert!(item2.get_int()? == 494);
        Ok(())
    }

    #[test]
    fn test_mul() -> VMResult<()> {
        let mut chunk = Chunk::new("no_file", 1);
        let line = 1;
        let const0 = chunk.add_constant(Value::Int(2 as i64)) as u16;
        let const1 = chunk.add_constant(Value::Int(3 as i64)) as u16;
        let const2 = chunk.add_constant(Value::Byte(1)) as u16;
        chunk.encode2(STORE_K, 0, const0, line)?;
        chunk.encode3(MUL_RK, 0, 0, const1, line).unwrap();
        chunk.encode3(MUL_RK, 0, 0, const2, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        vm.execute()?;
        assert!(vm.stack[0].get_int()? == 6);

        let mut chunk = Chunk::new("no_file", 1);
        let const0 = chunk.add_constant(Value::Float(5 as f64)) as u16;
        let const1 = chunk.add_constant(Value::Int(3 as i64)) as u16;
        let const2 = chunk.add_constant(Value::Byte(2)) as u16;
        chunk.encode2(STORE_K, 0, const0, line)?;
        chunk.encode3(MUL_RK, 0, 0, const1, line).unwrap();
        chunk.encode3(MUL_RK, 0, 0, const2, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        vm.execute()?;
        let item = vm.stack[0];
        assert!(!item.is_int());
        assert!(item.is_number());
        assert!(item.get_float()? == 30.0);

        let mut chunk = Chunk::new("no_file", 1);
        for i in 0..u16::MAX {
            chunk.add_constant(Value::Int(i as i64));
        }
        chunk.encode2(STORE_K, 1, 1, line)?;
        chunk.encode2(STORE_K, 2, 2, line)?;
        chunk.encode3(MUL, 0, 1, 2, line).unwrap();
        chunk.encode3(MUL_KR, 0, 5, 0, line).unwrap();
        chunk.encode3(MUL_KR, 1, 500, 0, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        vm.execute()?;
        let item = vm.stack[0];
        let item2 = vm.stack[1];
        assert!(item.is_int());
        assert!(item.get_int()? == 10);
        assert!(item2.get_int()? == 5000);
        Ok(())
    }

    #[test]
    fn test_div() -> VMResult<()> {
        let mut chunk = Chunk::new("no_file", 1);
        let line = 1;
        let const0 = chunk.add_constant(Value::Int(18 as i64)) as u16;
        let const1 = chunk.add_constant(Value::Int(2 as i64)) as u16;
        let const2 = chunk.add_constant(Value::Byte(3)) as u16;
        chunk.encode2(STORE_K, 0, const0, line)?;
        chunk.encode3(DIV_RK, 0, 0, const1, line).unwrap();
        chunk.encode3(DIV_RK, 0, 0, const2, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        vm.execute()?;
        assert!(vm.stack[0].get_int()? == 3);

        let mut chunk = Chunk::new("no_file", 1);
        let const0 = chunk.add_constant(Value::Float(10 as f64)) as u16;
        let const1 = chunk.add_constant(Value::Int(2 as i64)) as u16;
        let const2 = chunk.add_constant(Value::Byte(2)) as u16;
        chunk.encode2(STORE_K, 0, const0, line)?;
        chunk.encode3(DIV_RK, 0, 0, const1, line).unwrap();
        chunk.encode3(DIV_RK, 0, 0, const2, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        vm.execute()?;
        let item = vm.stack[0];
        assert!(!item.is_int());
        assert!(item.is_number());
        assert!(item.get_float()? == 2.5);

        let mut chunk = Chunk::new("no_file", 1);
        for i in 0..u16::MAX {
            chunk.add_constant(Value::Int(i as i64));
        }
        chunk.encode2(STORE_K, 1, 1, line)?;
        chunk.encode2(STORE_K, 2, 2, line)?;
        chunk.encode3(DIV, 0, 2, 1, line).unwrap();
        chunk.encode3(DIV_KR, 0, 10, 0, line).unwrap();
        chunk.encode3(DIV_KR, 1, 500, 0, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        vm.execute()?;
        let item = vm.stack[0];
        let item2 = vm.stack[1];
        assert!(item.is_int());
        assert!(item.get_int()? == 5);
        assert!(item2.get_int()? == 100);

        let mut chunk = Chunk::new("no_file", 1);
        let const0 = chunk.add_constant(Value::Int(10 as i64)) as u16;
        let const1 = chunk.add_constant(Value::Int(0 as i64)) as u16;
        chunk.encode2(STORE_K, 0, const0, line)?;
        chunk.encode2(STORE_K, 1, const1, line)?;
        chunk.encode3(DIV, 0, 0, 1, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        let res = vm.execute();
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string() == "[VM]: Divide by zero error.");

        let mut chunk = Chunk::new("no_file", 1);
        let const0 = chunk.add_constant(Value::Float(10 as f64)) as u16;
        let const1 = chunk.add_constant(Value::Float(0 as f64)) as u16;
        chunk.encode2(STORE_K, 0, const0, line)?;
        chunk.encode2(STORE_K, 1, const1, line)?;
        chunk.encode3(DIV, 0, 0, 1, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        let res = vm.execute();
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string() == "[VM]: Divide by zero error.");

        let mut chunk = Chunk::new("no_file", 1);
        let const0 = chunk.add_constant(Value::Float(10 as f64)) as u16;
        let const1 = chunk.add_constant(Value::Byte(0)) as u16;
        chunk.encode2(STORE_K, 0, const0, line)?;
        chunk.encode2(STORE_K, 1, const1, line)?;
        chunk.encode3(DIV, 0, 0, 1, line).unwrap();
        chunk.encode0(RET, line)?;
        let mut vm = Vm::new(chunk);
        let res = vm.execute();
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string() == "[VM]: Divide by zero error.");
        Ok(())
    }
}
