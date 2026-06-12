mod rule;

use crate::optimize_amine::rule::RULES;
use amine_asm::instruction::Instruction;
use std::collections::LinkedList;

pub fn minimize_amine(code: Vec<Instruction>) -> Vec<Instruction> {
    // only move forward if no pattern matches
    let mut linked_list: LinkedList<_> = code.into_iter().collect();
    let mut cursor = linked_list.cursor_front_mut();
    while cursor.current().is_some() {
        while RULES.iter().any(|rule| rule(&mut cursor)) {}
        cursor.move_next();
    }
    linked_list.into_iter().collect()
}
