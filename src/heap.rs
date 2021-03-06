use std::borrow::Cow;

use crate::error::*;
use crate::value::*;

const FLAG_MARK: u8 = 0x01;
const FLAG_TRACE: u8 = 0x02;
const FLAG_STICKY: u8 = 0x04;
//const FLAG_NEW: u8 = 0x08;

const TYPE_VALUE: u8 = 0x00;
const TYPE_STRING: u8 = 0x10;
const TYPE_VECTOR: u8 = 0x20;
const TYPE_BYTES: u8 = 0x30;
const TYPE_PAIR: u8 = 0x40;

macro_rules! is_bit_set {
    ($val:expr, $bit:expr) => {{
        ($val & $bit) != 0
    }};
}

macro_rules! set_bit {
    ($val:expr, $bit:expr) => {{
        $val |= $bit;
    }};
}

macro_rules! clear_bit {
    ($val:expr, $bit:expr) => {{
        if is_bit_set!($val, $bit) {
            $val ^= $bit;
        }
    }};
}

fn is_marked(flag: u8) -> bool {
    is_bit_set!(flag, FLAG_MARK | FLAG_STICKY)
}

fn need_trace(flag: u8) -> bool {
    is_marked(flag) && is_bit_set!(flag, FLAG_TRACE)
}

// This is anything that can live on the heap.  Values normally live on the
// stack or as constants but can be stored in the heap as well
// (for instance closed over values or globals).
#[derive(Clone, Debug)]
pub enum Object {
    Value(Value),
    String(Cow<'static, str>),
    Vector(Vec<Value>),
    Bytes(Vec<u8>),
    Pair(Value, Value),
}

impl Object {
    pub fn is_nil(&self) -> bool {
        matches!(self, Object::Value(Value::Nil))
    }
}

pub type HandleRef<'a> = &'a Object;
pub type HandleRefMut<'a> = &'a mut Object;

#[derive(Copy, Clone, Debug)]
pub struct Handle {
    idx: usize,
}

pub struct HeapStats {
    live_objects: usize,
    sticky_objects: usize,
    //string_bytes: usize,
    //vec_bytes: usize,
    //byte_bytes: usize,
}

impl HeapStats {
    pub fn new() -> Self {
        HeapStats {
            live_objects: 0,
            sticky_objects: 0,
            //string_bytes: 0,
            //vec_bytes: 0,
            //byte_bytes: 0,
        }
    }

    pub fn live_objects(&self) -> usize {
        self.live_objects
    }
}

impl Default for HeapStats {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Heap {
    flags: Vec<u8>,
    objects: Vec<Object>,
    greys: Vec<usize>,
    grow_factor: f64,
    capacity: usize,
    stats: HeapStats,
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

impl Heap {
    pub fn new() -> Self {
        Heap {
            flags: Vec::with_capacity(512),
            // Keep one extra slot to do sway on replace.
            objects: Vec::with_capacity(512 + 1),
            greys: vec![],
            grow_factor: 2.0,
            capacity: 512,
            stats: HeapStats::default(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Heap {
            flags: Vec::with_capacity(capacity),
            // Keep one extra slot to do sway on replace.
            objects: Vec::with_capacity(capacity + 1),
            greys: vec![],
            grow_factor: 2.0,
            capacity,
            stats: HeapStats::default(),
        }
    }

    pub fn set_grow_factor(&mut self, grow_factor: f64) {
        self.grow_factor = grow_factor;
    }

    fn type_flag(obj: &Object) -> u8 {
        match obj {
            Object::Value(Value::Reference(_)) => TYPE_VALUE | FLAG_TRACE,
            Object::Value(_) => TYPE_VALUE,
            Object::String(_) => TYPE_STRING,
            Object::Vector(_) => TYPE_VECTOR | FLAG_TRACE,
            Object::Bytes(_) => TYPE_BYTES,
            Object::Pair(_, _) => TYPE_PAIR | FLAG_TRACE,
        }
    }

    pub fn alloc<MarkFunc>(&mut self, obj: Object, mark_roots: MarkFunc) -> Handle
    where
        MarkFunc: Fn(&mut Heap) -> VMResult<()>,
    {
        if self.stats.live_objects() >= self.capacity() {
            if let Err(err) = self.collect(mark_roots) {
                panic!("Garbage collection failed! {}", err);
            }
            let new_min = (self.stats.live_objects() as f64 * self.grow_factor) as usize;
            if new_min > self.capacity() {
                self.capacity = new_min;
                self.flags.reserve(new_min - self.flags.len());
                self.objects.reserve((new_min - self.objects.len()) + 1);
            }
        }
        if self.objects.len() < self.capacity() {
            let type_flag = Self::type_flag(&obj);
            let idx = self.objects.len();
            self.objects.push(obj);
            self.flags.push(type_flag | FLAG_MARK);
            self.stats.live_objects += 1;
            Handle { idx }
        } else {
            for (idx, flag) in self.flags.iter_mut().enumerate() {
                if !is_marked(*flag) {
                    self.stats.live_objects += 1;
                    let type_flag = Self::type_flag(&obj);
                    *flag = type_flag | FLAG_MARK;
                    self.objects.push(obj);
                    self.objects.swap_remove(idx);
                    return Handle { idx };
                }
            }
            panic!("Failed to allocate to heap- no free objects and no capacity!");
        }
    }

    pub fn is_value(&self, handle: Handle) -> bool {
        if let Some(flag) = self.flags.get(handle.idx) {
            (flag & 0xf0) == TYPE_VALUE
        } else {
            false
        }
    }

    pub fn is_string(&self, handle: Handle) -> bool {
        if let Some(flag) = self.flags.get(handle.idx) {
            (flag & 0xf0) == TYPE_STRING
        } else {
            false
        }
    }

    pub fn is_vector(&self, handle: Handle) -> bool {
        if let Some(flag) = self.flags.get(handle.idx) {
            (flag & 0xf0) == TYPE_VECTOR
        } else {
            false
        }
    }

    pub fn is_bytes(&self, handle: Handle) -> bool {
        if let Some(flag) = self.flags.get(handle.idx) {
            (flag & 0xf0) == TYPE_BYTES
        } else {
            false
        }
    }

    pub fn is_pair(&self, handle: Handle) -> bool {
        if let Some(flag) = self.flags.get(handle.idx) {
            (flag & 0xf0) == TYPE_PAIR
        } else {
            false
        }
    }

    pub fn get(&self, handle: Handle) -> VMResult<HandleRef<'_>> {
        if let Some(data) = self.objects.get(handle.idx) {
            Ok(data)
        } else {
            Err(VMError::new_heap("Invalid object handle!"))
        }
    }

    pub fn get_mut(&mut self, handle: Handle) -> VMResult<HandleRefMut<'_>> {
        if let Some(data) = self.objects.get_mut(handle.idx) {
            Ok(data)
        } else {
            Err(VMError::new_heap("Invalid object handle!"))
        }
    }

    pub fn replace(&mut self, handle: Handle, obj: Object) -> VMResult<Object> {
        let type_flag = Self::type_flag(&obj);
        self.objects.push(obj);
        let old = self.objects.swap_remove(handle.idx);
        self.flags[handle.idx] = type_flag | (self.flags[handle.idx] & 0x0f);
        Ok(old)
    }

    pub fn get_value(&self, handle: Handle) -> VMResult<Value> {
        if let Object::Value(val) = self.get(handle)? {
            Ok(*val)
        } else {
            Ok(Value::Reference(handle))
        }
    }

    pub fn is_marked(&self, handle: Handle) -> bool {
        if let Some(flag) = self.flags.get(handle.idx) {
            is_marked(*flag)
        } else {
            false
        }
    }

    pub fn mark(&mut self, handle: Handle) -> VMResult<()> {
        if let Some(flag) = self.flags.get_mut(handle.idx) {
            if !is_marked(*flag) {
                self.stats.live_objects += 1;
                set_bit!(*flag, FLAG_MARK);
            }
            Ok(())
        } else {
            Err(VMError::new_heap("Invalid object handle!"))
        }
    }

    pub fn sticky(&mut self, handle: Handle) -> VMResult<()> {
        if let Some(flag) = self.flags.get_mut(handle.idx) {
            if !is_bit_set!(*flag, FLAG_STICKY) {
                self.stats.sticky_objects += 1;
                self.stats.live_objects += 1;
                set_bit!(*flag, FLAG_STICKY);
            }
            Ok(())
        } else {
            Err(VMError::new_heap("Invalid object handle!"))
        }
    }

    pub fn unsticky(&mut self, handle: Handle) -> VMResult<()> {
        if let Some(flag) = self.flags.get_mut(handle.idx) {
            if is_bit_set!(*flag, FLAG_STICKY) {
                self.stats.sticky_objects -= 1;
                self.stats.live_objects -= 1;
                clear_bit!(*flag, FLAG_STICKY);
            }
            Ok(())
        } else {
            Err(VMError::new_heap("Invalid object handle!"))
        }
    }

    // mark_trace has an invariant to maintain, do not touch objects (see unsafe in
    // trace below).
    fn mark_trace(&mut self, handle: Handle, current: usize) -> VMResult<()> {
        if !self.is_marked(handle) {
            self.mark(handle)?;
            if handle.idx < current {
                self.greys.push(handle.idx);
            }
        }
        Ok(())
    }

    fn trace(&mut self, idx: usize, current: usize) -> VMResult<()> {
        // This unsafe avoids cloning the object to avoid having a mutable and immutable self.
        // This should be fine because we are not touching objects in a mark, only flags.
        // idx should also have been validated before it gets here (by mark if nothing else).
        let obj = unsafe { &*(self.objects.get_unchecked(idx) as *const Object) };
        match obj {
            Object::Value(Value::Reference(h)) => self.mark_trace(*h, current)?,
            Object::Value(_) => {}
            Object::String(_) => {}
            Object::Vector(vec) => {
                for v in vec {
                    if let Value::Reference(h) = v {
                        let h = *h;
                        self.mark_trace(h, current)?;
                    }
                }
            }
            Object::Bytes(_) => {}
            Object::Pair(Value::Reference(car), Value::Reference(cdr)) => {
                self.mark_trace(*car, current)?;
                self.mark_trace(*cdr, current)?;
            }
            Object::Pair(Value::Reference(car), _) => self.mark_trace(*car, current)?,
            Object::Pair(_, Value::Reference(cdr)) => self.mark_trace(*cdr, current)?,
            Object::Pair(_, _) => {}
        }
        Ok(())
    }

    pub fn collect<MarkFunc>(&mut self, mark_roots: MarkFunc) -> VMResult<()>
    where
        MarkFunc: Fn(&mut Heap) -> VMResult<()>,
    {
        self.stats.live_objects = self.stats.sticky_objects;
        for flag in self.flags.iter_mut() {
            clear_bit!(*flag, FLAG_MARK);
        }
        mark_roots(self)?;
        let mut cur = 0;
        //for (cur, flag) in self.flags.iter().enumerate() {
        let mut val = self.flags.get(cur);
        while let Some(flag) = val {
            if need_trace(*flag) {
                self.trace(cur, cur)?;
                while let Some(idx) = self.greys.pop() {
                    self.trace(idx, cur)?;
                }
            }
            cur += 1;
            val = self.flags.get(cur);
        }
        Ok(())
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn live_objects(&self) -> usize {
        self.stats.live_objects()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic() -> VMResult<()> {
        let mut heap = Heap::default();
        let mark_roots = |_heap: &mut Heap| -> VMResult<()> { Ok(()) };
        assert!(heap.capacity() == 512);
        assert!(heap.live_objects() == 0);
        for x in 0..512 {
            heap.alloc(Object::Value(Value::Int(x)), mark_roots);
        }
        assert!(heap.capacity() == 512);
        assert!(heap.live_objects() == 512);
        for x in 0..512 {
            let obj = heap.get(Handle { idx: x }).unwrap();
            if let Object::Value(Value::Int(v)) = obj {
                assert!(x == *v as usize);
            } else {
                assert!(false);
            }
        }
        heap.alloc(Object::Value(Value::Int(512)), mark_roots);
        assert!(heap.capacity() == 512);
        assert!(heap.live_objects() == 1);
        let obj = heap.get(Handle { idx: 0 }).unwrap();
        if let Object::Value(Value::Int(v)) = obj {
            assert!(512 == *v);
        } else {
            assert!(false);
        }
        let mark_roots = |heap: &mut Heap| -> VMResult<()> {
            for idx in 0..512 {
                heap.mark(Handle { idx })?;
            }
            Ok(())
        };
        for x in 0..512 {
            heap.alloc(Object::Value(Value::Int(x)), mark_roots);
        }
        assert!(heap.capacity() == 1024);
        assert!(heap.live_objects() == 513);
        for x in 0..513 {
            let obj = heap.get(Handle { idx: x }).unwrap();
            if let Object::Value(Value::Int(v)) = obj {
                if x == 0 {
                    assert!(512 == *v);
                } else {
                    assert!(x - 1 == *v as usize);
                }
            } else {
                assert!(false);
            }
        }
        let mark_roots = |heap: &mut Heap| -> VMResult<()> {
            for idx in 0..513 {
                if idx % 2 == 0 {
                    heap.mark(Handle { idx })?;
                }
            }
            Ok(())
        };
        heap.collect(mark_roots)?;
        assert!(heap.capacity() == 1024);
        assert!(heap.live_objects() == 257);
        let mark_roots = |_heap: &mut Heap| -> VMResult<()> { Ok(()) };
        heap.collect(mark_roots)?;
        assert!(heap.capacity() == 1024);
        assert!(heap.live_objects() == 0);
        for x in 0..512 {
            let h = heap.alloc(Object::Value(Value::Int(x)), mark_roots);
            heap.sticky(h)?;
        }
        heap.collect(mark_roots)?;
        assert!(heap.capacity() == 1024);
        assert!(heap.live_objects() == 512);
        for x in 512..1024 {
            let _h = heap.alloc(Object::Value(Value::Int(x)), mark_roots);
        }
        let mark_roots = |heap: &mut Heap| -> VMResult<()> {
            for idx in 0..1024 {
                heap.mark(Handle { idx })?;
            }
            Ok(())
        };
        heap.collect(mark_roots)?;
        assert!(heap.capacity() == 1024);
        assert!(heap.live_objects() == 1024);
        heap.alloc(Object::String("steve".into()), mark_roots);
        assert!(heap.capacity() == 2048);
        assert!(heap.live_objects() == 1025);
        Ok(())
    }

    #[test]
    fn test_trace_val() -> VMResult<()> {
        let mut heap = Heap::default();

        assert!(heap.capacity() == 512);
        assert!(heap.live_objects() == 0);
        let outers = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
        let outers_mark = outers.clone();
        let mark_roots = |heap: &mut Heap| -> VMResult<()> {
            for h in outers_mark.borrow().iter() {
                heap.mark(*h)?;
            }
            Ok(())
        };
        for x in 0..256 {
            let inner = heap.alloc(Object::Value(Value::Int(x)), mark_roots);
            outers
                .borrow_mut()
                .push(heap.alloc(Object::Value(Value::Reference(inner)), mark_roots));
        }
        assert!(heap.capacity() == 512);
        assert!(heap.live_objects() == 512);
        let mut i = 0;
        for h in outers.borrow().iter() {
            let obj = heap.get(*h).unwrap();
            if let Object::Value(Value::Reference(inner)) = obj {
                let obj = heap.get(*inner).unwrap();
                if let Object::Value(Value::Int(v)) = obj {
                    assert!(i == *v as usize);
                } else {
                    assert!(false);
                }
            } else {
                assert!(false);
            }
            i += 1;
        }
        heap.collect(mark_roots)?;
        assert!(heap.capacity() == 512);
        assert!(heap.live_objects() == 512);
        for h in outers.borrow().iter() {
            heap.replace(*h, Object::String("bloop".into()))?;
        }
        heap.collect(mark_roots)?;
        assert!(heap.capacity() == 512);
        assert!(heap.live_objects() == 256);
        for h in outers.borrow().iter() {
            let obj = heap.get(*h).unwrap();
            if let Object::String(sstr) = obj {
                assert!(sstr == "bloop");
            } else {
                assert!(false);
            }
            i += 1;
        }
        Ok(())
    }

    #[test]
    fn test_trace_vec() -> VMResult<()> {
        let mut heap = Heap::default();

        assert!(heap.capacity() == 512);
        assert!(heap.live_objects() == 0);
        let outers = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
        let outers_mark = outers.clone();
        let mark_roots = |heap: &mut Heap| -> VMResult<()> {
            for h in outers_mark.borrow().iter() {
                heap.mark(*h)?;
            }
            Ok(())
        };
        let mut v = vec![];
        for x in 0..256 {
            let inner = heap.alloc(Object::Value(Value::Int(x)), mark_roots);
            v.push(Value::Reference(inner));
        }
        outers
            .borrow_mut()
            .push(heap.alloc(Object::Vector(v), mark_roots));
        assert!(heap.capacity() == 512);
        assert!(heap.live_objects() == 257);
        for h in outers.borrow().iter() {
            let obj = heap.get(*h).unwrap();
            if let Object::Vector(v) = obj {
                let mut i = 0;
                for hv in v {
                    if let Value::Reference(hv) = hv {
                        let obj = heap.get(*hv).unwrap();
                        if let Object::Value(Value::Int(v)) = obj {
                            assert!(i == *v as usize);
                        } else {
                            assert!(false);
                        }
                    } else {
                        assert!(false);
                    }
                    i += 1;
                }
            } else {
                assert!(false);
            }
        }
        heap.collect(mark_roots)?;
        assert!(heap.capacity() == 512);
        assert!(heap.live_objects() == 257);
        for h in outers.borrow().iter() {
            let obj = heap.get(*h).unwrap();
            if let Object::Vector(v) = obj {
                let mut i = 0;
                for hv in v {
                    if let Value::Reference(hv) = hv {
                        let obj = heap.get(*hv).unwrap();
                        if let Object::Value(Value::Int(v)) = obj {
                            assert!(i == *v as usize);
                        } else {
                            assert!(false);
                        }
                    } else {
                        assert!(false);
                    }
                    i += 1;
                }
            } else {
                assert!(false);
            }
        }
        for h in outers.borrow().iter() {
            heap.replace(*h, Object::String("bloop".into()))?;
        }
        heap.collect(mark_roots)?;
        assert!(heap.capacity() == 512);
        assert!(heap.live_objects() == 1);
        for h in outers.borrow().iter() {
            let obj = heap.get(*h).unwrap();
            if let Object::String(sstr) = obj {
                assert!(sstr == "bloop");
            } else {
                assert!(false);
            }
        }
        Ok(())
    }

    #[test]
    fn test_trace_pair() -> VMResult<()> {
        let mut heap = Heap::default();
        assert!(heap.capacity() == 512);
        assert!(heap.live_objects() == 0);
        let outers = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
        let outers_mark = outers.clone();
        let mark_roots = |heap: &mut Heap| -> VMResult<()> {
            for h in outers_mark.borrow().iter() {
                heap.mark(*h)?;
            }
            Ok(())
        };
        outers
            .borrow_mut()
            .push(heap.alloc(Object::Pair(Value::Int(1), Value::Int(2)), mark_roots));
        let car_h = heap.alloc(Object::Value(Value::Int(3)), mark_roots);
        let cdr_h = heap.alloc(Object::Value(Value::Int(4)), mark_roots);
        outers.borrow_mut().push(heap.alloc(
            Object::Pair(Value::Reference(car_h), Value::Int(2)),
            mark_roots,
        ));
        outers.borrow_mut().push(heap.alloc(
            Object::Pair(Value::Int(1), Value::Reference(cdr_h)),
            mark_roots,
        ));
        outers.borrow_mut().push(heap.alloc(
            Object::Pair(Value::Reference(car_h), Value::Reference(cdr_h)),
            mark_roots,
        ));
        assert!(heap.capacity() == 512);
        assert!(heap.live_objects() == 6);
        heap.collect(mark_roots)?;
        assert!(heap.capacity() == 512);
        assert!(heap.live_objects() == 6);
        let mut i = 0;
        for h in outers.borrow().iter() {
            let obj = heap.get(*h).unwrap();
            if let Object::Pair(car, cdr) = obj {
                if i == 0 {
                    let (car, cdr) = if let Value::Int(car) = car {
                        if let Value::Int(cdr) = cdr {
                            (*car, *cdr)
                        } else {
                            (*car, 0)
                        }
                    } else {
                        (0, 0)
                    };
                    assert!(car == 1);
                    assert!(cdr == 2);
                } else if i == 1 {
                    let (car, cdr) = if let Value::Reference(car_h) = car {
                        if let Object::Value(Value::Int(car)) = heap.get(*car_h).unwrap() {
                            if let Value::Int(cdr) = cdr {
                                (*car, *cdr)
                            } else {
                                (*car, 0)
                            }
                        } else {
                            (0, 0)
                        }
                    } else {
                        (0, 0)
                    };
                    assert!(car == 3);
                    assert!(cdr == 2);
                } else if i == 2 {
                    let (car, cdr) = if let Value::Reference(cdr_h) = cdr {
                        if let Object::Value(Value::Int(cdr)) = heap.get(*cdr_h).unwrap() {
                            if let Value::Int(car) = car {
                                (*car, *cdr)
                            } else {
                                (0, *cdr)
                            }
                        } else {
                            (0, 0)
                        }
                    } else {
                        (0, 0)
                    };
                    assert!(car == 1);
                    assert!(cdr == 4);
                } else if i == 3 {
                    let (car, cdr) = if let Value::Reference(car_h) = car {
                        if let Object::Value(Value::Int(car)) = heap.get(*car_h).unwrap() {
                            if let Value::Reference(cdr_h) = cdr {
                                if let Object::Value(Value::Int(cdr)) = heap.get(*cdr_h).unwrap() {
                                    (*car, *cdr)
                                } else {
                                    (*car, 0)
                                }
                            } else {
                                (0, 0)
                            }
                        } else {
                            (0, 0)
                        }
                    } else {
                        (0, 0)
                    };
                    assert!(car == 3);
                    assert!(cdr == 4);
                } else {
                    assert!(false);
                }
            } else {
                assert!(false);
            }
            i += 1;
        }

        Ok(())
    }
}
