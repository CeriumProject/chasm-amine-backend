use amine_asm::instruction::{Instruction as AI, SingleOpOpcode as ASOO, TwoOpOpcode as ATOO};
use amine_asm::operand::{RawRegOp, RegOp};
use std::collections::linked_list::CursorMut;

pub(super) type OptimizationRule = fn(&mut CursorMut<AI>) -> bool;

pub(super) const RULES: &[OptimizationRule] = &[join_add, join_push, join_pop];

fn join_add(cursor: &mut CursorMut<AI>) -> bool {
    let Some(AI::TwoOp(ATOO::Add, curr_op, RegOp::Direct(RawRegOp::Value(curr_val)))) =
        cursor.current()
    else {
        return false;
    };
    let curr_val = *curr_val;
    let curr_op = curr_op.clone();
    let Some(AI::TwoOp(ATOO::Add, next_op, RegOp::Direct(RawRegOp::Value(next_val)))) =
        cursor.peek_next()
    else {
        return false;
    };
    if curr_op != *next_op {
        return false;
    }
    *next_val += curr_val;
    cursor.remove_current();
    true
}

fn join_push(cursor: &mut CursorMut<AI>) -> bool {
    let Some(AI::SingleOp(ASOO::Push, lhs_op)) = cursor.current() else {
        return false;
    };
    let lhs_op = lhs_op.clone();
    let Some(AI::SingleOp(ASOO::Push, rhs_op)) = cursor.peek_next() else {
        return false;
    };
    let rhs_op = rhs_op.clone();
    cursor.remove_current();
    cursor.remove_current();
    cursor.insert_before(AI::TwoOp(ATOO::PushT, lhs_op, rhs_op));
    true
}

fn join_pop(cursor: &mut CursorMut<AI>) -> bool {
    let Some(AI::SingleOp(ASOO::Pop, rhs_op)) = cursor.current() else {
        return false;
    };
    let rhs_op = rhs_op.clone();
    let Some(AI::SingleOp(ASOO::Pop, lhs_op)) = cursor.peek_next() else {
        return false;
    };
    let lhs_op = lhs_op.clone();
    cursor.remove_current();
    cursor.remove_current();
    cursor.insert_before(AI::TwoOp(ATOO::PopT, lhs_op, rhs_op));
    true
}
