use std::sync::Arc;

use slvm::chunk::*;
use slvm::error::*;
use slvm::opcodes::*;
use slvm::value::*;
use slvm::vm::*;

fn main() -> Result<(), VMError> {
    // algorithm from http://dan.corlan.net/bench.html
    // Do a lot of loops and simple math and to see how we stack up.
    /*
    (defn eval-pol (n x)
      (let ((su 0.0) (mu 10.0) (pu 0.0)
            (pol (make-vec 100 0.0)))
        (dotimes-i i n
          (do
            (set! su 0.0)
            (dotimes-i j 100
               (do
                 (set! mu (/ (+ mu 2.0) 2.0))
                 (vec-set! pol j mu)))
            (dotimes-i j 100
              (set! su (+ (vec-nth pol j) (* su x))))
            (set! pu (+ pu su))))
        (println pu)))
             */
    let mut vm = Vm::new();
    let mut chunk = Chunk::new("no_file", 1);
    let n = chunk.add_constant(Value::Int(500_000)) as u16;
    let x = chunk.add_constant(Value::float(0.2)) as u16;
    let su = chunk.add_constant(Value::float(0.0)) as u16;
    let mu = chunk.add_constant(Value::float(10.0)) as u16;
    let pu = chunk.add_constant(Value::float(0.0)) as u16;
    let zero = chunk.add_constant(Value::Int(0)) as u16;
    let zerof = chunk.add_constant(Value::float(0.0)) as u16;
    let twof = chunk.add_constant(Value::float(2.0)) as u16;
    let hundred = chunk.add_constant(Value::Int(100)) as u16;
    let one = chunk.add_constant(Value::Int(1)) as u16;
    chunk.encode2(CONST, 1, n, Some(1))?;
    chunk.encode2(CONST, 2, x, None)?;
    chunk.encode2(CONST, 3, su, None)?;
    chunk.encode2(CONST, 4, mu, None)?;
    chunk.encode2(CONST, 5, pu, None)?;
    chunk.encode2(CONST, 6, zero, None)?; // i
    chunk.encode2(CONST, 7, zero, None)?; // j
    chunk.encode2(CONST, 8, twof, None)?; // 2.0
    chunk.encode2(CONST, 100, hundred, None)?;
    chunk.encode2(CONST, 101, one, None)?;
    chunk.encode2(CONST, 103, zerof, None)?;

    chunk.encode3(VECMKD, 10, 100, 103, None)?; // pols
                                                //chunk.encode2(VECELS, 10, 100, None)?;
                                                // loop i .. n
    chunk.encode2(CONST, 3, zerof, None)?;
    chunk.encode2(CONST, 7, zero, None)?; // j
                                          // loop j .. 100
                                          // (set! mu (/ (+ mu 2.0) 2.0))
    chunk.encode3(ADD, 4, 4, 8, None)?;
    chunk.encode3(DIV, 4, 4, 8, None)?;
    // (vec-set! pol j mu)))
    chunk.encode3(VECSTH, 10, 4, 7, None)?;

    chunk.encode2(INC, 7, 1, None)?;
    chunk.encode3(JMPLT, 7, 100, 0x2b, None)?;

    chunk.encode2(CONST, 7, zero, None)?; // j
                                          // (dotimes-i j 100 (j2)
                                          //   (set! su (+ (vec-nth pol j) (* su x))))
    chunk.encode3(MUL, 50, 3, 2, None)?;
    chunk.encode3(VECNTH, 10, 51, 7, None)?;
    chunk.encode3(ADD, 3, 50, 51, None)?;

    chunk.encode2(INC, 7, 1, None)?;
    chunk.encode3(JMPLT, 7, 100, 0x41, None)?;
    // (set! pu (+ pu su))))
    chunk.encode3(ADD, 5, 5, 3, None)?;

    chunk.encode2(INC, 6, 1, None)?;
    chunk.encode3(JMPLT, 6, 1, 0x25, None)?;

    chunk.encode0(RET, None)?;

    //chunk.disassemble_chunk()?;
    //assert!(false);

    let chunk = Arc::new(chunk);
    vm.execute(chunk)?;
    let result = vm.get_stack(5).get_float()?;
    println!("{}", result);

    Ok(())
}