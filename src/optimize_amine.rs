use amine_asm::instruction::Instruction;

pub fn minimize_amine(mut code: Vec<Instruction>) -> Vec<Instruction> {
    // only move forward if no pattern matches
    let mut index = 0;
    while index < code.len() {
        // TODO
        index += 1;
    }
    code
}