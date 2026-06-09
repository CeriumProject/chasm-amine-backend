use chasm_ir::{Instruction, Operand};

pub(crate) fn minimize(instruction: chasm_ir::Instruction) -> Option<chasm_ir::Instruction> {
    use chasm_ir::TwoOpOpcode as TOO;

    match instruction {
        Instruction::Alloc(name, size, body) => {
            // TODO: fuse redundant allocations
            let body = body.into_iter().flat_map(minimize).collect();
            Some(Instruction::Alloc(name, size, body))
        }
        Instruction::Param(name, size, body) => {
            let body = body.into_iter().flat_map(minimize).collect();
            Some(Instruction::Alloc(name, size, body))
        }
        Instruction::Result(name, size, body) => {
            let body = body.into_iter().flat_map(minimize).collect();
            Some(Instruction::Alloc(name, size, body))
        }
        Instruction::TwoOp(TOO::Add | TOO::Sub | TOO::Shl | TOO::Shr, _, Operand::Constant(0)) => None,
        Instruction::TwoOp(TOO::Mov | TOO::Sub, Operand::Variable(lhs), Operand::Variable(rhs)) if lhs == rhs => None,
        _ => Some(instruction),
    }
}