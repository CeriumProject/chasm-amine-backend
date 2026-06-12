#![feature(linked_list_cursors)]

mod optimize_amine;
mod optimize_chasm;

use crate::optimize_amine::minimize_amine;
use crate::optimize_chasm::minimize;
use amine_asm::instruction::Instruction as AmineInstruction;
use amine_asm::opcode::{NoOpOpcode as ANOO, SingleOpOpcode as ASOO, TwoOpOpcode as ATOO};
use amine_asm::operand::{RawRegOp, RegOp, Register};
use chasm_ir::iter::IntoInstIter;
use chasm_ir::{
    Instruction as ChasmInstruction, NoOpOpcode as CNOO, NoOpOpcode, Operand,
    SingleOpOpcode as CSOO, SingleOpOpcode, TwoOpOpcode as CTOO, TwoOpOpcode,
};
use std::collections::HashMap;
use std::vec::IntoIter;

enum AbstractAddress {
    OuterParam(usize),
    Volatile(usize),
    StackVar(usize),
    RegVar(usize),
    InnerParam(usize),
    OuterResult(usize),
}

struct Allocation {
    name: String,
    address: AbstractAddress,
}

pub fn compile_chasm_to_amine(ir: &[chasm_ir::Section]) -> Vec<AmineInstruction> {
    let mut result = vec![
        AmineInstruction::TwoOp(
            ATOO::Mov,
            RegOp::Direct(RawRegOp::Register(Register::RS)),
            RegOp::Direct(RawRegOp::Const(String::from("§STACK"))),
        ),
        AmineInstruction::SingleOp(
            ASOO::Call,
            RegOp::Direct(RawRegOp::Const(String::from("main"))),
        ),
        AmineInstruction::NoOp(ANOO::Exit),
    ];
    result.extend(ir.iter().flat_map(compile_section));
    result.push(AmineInstruction::Label(String::from("§STACK")));
    result
}

/// [previous stack] [stack frame data] | [results] [stack vars] [stored regs] [parameters]
fn compile_section(section: &chasm_ir::Section) -> Vec<AmineInstruction> {
    let mut allocations = Vec::new();
    determine_allocations(
        &section.body,
        &mut MemoryConfig::default(),
        &mut allocations,
    );
    let offsets = find_offsets(&allocations);
    let mut allocations = allocations.into_iter();
    let mut vars = Vars::new(
        section
            .signature
            .as_ref()
            .unwrap()
            .1
            .iter()
            .map(|(name, _)| name.to_owned())
            .collect(),
        epilogue(&offsets),
    );
    let body = section
        .body
        .iter()
        .cloned()
        .flat_map(minimize)
        .flat_map(|inst| compile_inst(&inst, &mut vars, &mut allocations, offsets.1.clone()));
    let unminimized = vec![AmineInstruction::Label(section.name.to_owned())]
        .into_iter()
        .chain(prologue(&offsets))
        .chain(body)
        .chain(vec![AmineInstruction::Blank])
        .collect();
    minimize_amine(unminimized)
}

fn prologue(offsets: &(usize, (usize, usize, usize))) -> Vec<AmineInstruction> {
    let mut result = vec![AmineInstruction::TwoOp(
        ATOO::Add,
        RegOp::Direct(RawRegOp::Register(Register::RS)),
        RegOp::Direct(RawRegOp::Value(offsets.1.1 as u16)),
    )];
    for _reg_var in 0..offsets.0 {
        result.push(AmineInstruction::SingleOp(
            ASOO::Push,
            RegOp::Direct(RawRegOp::Register(idx_to_reg(_reg_var + 2).unwrap())),
        ))
    }
    result
}

fn epilogue(offsets: &(usize, (usize, usize, usize))) -> Vec<AmineInstruction> {
    let mut result = Vec::new();
    for _reg_var in (0..offsets.0).rev() {
        result.push(AmineInstruction::SingleOp(
            ASOO::Pop,
            RegOp::Direct(RawRegOp::Register(idx_to_reg(_reg_var + 2).unwrap())),
        ))
    }
    result
}

fn compile_inst(
    inst: &ChasmInstruction,
    vars: &mut Vars,
    allocations: &mut IntoIter<Allocation>,
    offsets: (usize, usize, usize),
) -> Vec<AmineInstruction> {
    match inst {
        ChasmInstruction::Sublabel(label) => vec![AmineInstruction::Sublabel(label.to_string())],
        // worst code ever?
        ChasmInstruction::Alloc(name, _, body)
        | ChasmInstruction::Param(name, _, body)
        | ChasmInstruction::Result(name, _, body) => {
            let is_param = matches!(inst, ChasmInstruction::Param(_, _, _));
            let mut result = Vec::new();
            vars.push_scope();
            vars.push_var(allocations.next().unwrap(), offsets);
            if is_param {
                vars.param_depth += 1;
                result.push(AmineInstruction::SingleOp(
                    ASOO::Inc,
                    RegOp::Direct(RawRegOp::Register(Register::RS)),
                ));
            }
            result.extend(
                body.into_iter()
                    .flat_map(|a| compile_inst(a, vars, allocations, offsets)),
            );
            vars.pop_scope();
            if is_param {
                vars.param_depth -= 1;
                result.push(AmineInstruction::SingleOp(
                    ASOO::Dec,
                    RegOp::Direct(RawRegOp::Register(Register::RS)),
                ));
            }
            result
        }
        ChasmInstruction::Reference(dst, src) => {
            let op = vars.lookup(src);
            let RegOp::Indirect(RawRegOp::Value(base_offset)) = op else {
                panic!()
            };
            vec![
                AmineInstruction::TwoOp(
                    ATOO::Mov,
                    vars.lookup_operand(&dst),
                    RegOp::Direct(RawRegOp::Register(Register::RB)),
                ),
                AmineInstruction::TwoOp(
                    ATOO::Add,
                    vars.lookup_operand(&dst),
                    RegOp::Direct(RawRegOp::Value(base_offset)),
                ),
            ]
        }
        ChasmInstruction::Receive(dst, idx) => vec![
            AmineInstruction::TwoOp(
                ATOO::Mov,
                vars.lookup_operand(&dst),
                RegOp::Direct(RawRegOp::Register(Register::RS)),
            ),
            AmineInstruction::TwoOp(
                ATOO::Lookup,
                vars.lookup_operand(&dst),
                RegOp::Direct(RawRegOp::Value(*idx as u16 + 2)),
            ),
        ],
        ChasmInstruction::TwoOp(opcode, op1, op2) => compile_two_op(opcode, op1, op2, vars),
        ChasmInstruction::SingleOp(opcode, op) => compile_single_op(opcode, op, vars),
        ChasmInstruction::NoOp(opcode) => compile_no_op(opcode, vars),
    }
}

fn compile_two_op(
    opcode: &CTOO,
    op1: &Operand,
    op2: &Operand,
    vars: &Vars,
) -> Vec<AmineInstruction> {
    use AmineInstruction::TwoOp as AITO;
    let opcode = match opcode {
        TwoOpOpcode::Mov => ATOO::Mov,
        TwoOpOpcode::Read => ATOO::Read,
        TwoOpOpcode::Write => ATOO::Write,
        TwoOpOpcode::Copy => ATOO::Copy,
        TwoOpOpcode::Add => ATOO::Add,
        TwoOpOpcode::Sub => ATOO::Sub,
        TwoOpOpcode::Mul => ATOO::Mul,
        TwoOpOpcode::Div => ATOO::Div,
        TwoOpOpcode::Jrnzdec => ATOO::JrnzDec,
        TwoOpOpcode::Lookup => ATOO::Lookup,
        TwoOpOpcode::Fadd => ATOO::Fadd,
        TwoOpOpcode::Fsub => ATOO::Fsub,
        TwoOpOpcode::Fmul => ATOO::Fmul,
        TwoOpOpcode::Fdiv => ATOO::Fdiv,
        TwoOpOpcode::Imul => ATOO::Imul,
        TwoOpOpcode::Idiv => ATOO::Idiv,
        TwoOpOpcode::Shr => ATOO::Shr,
        TwoOpOpcode::Shl => ATOO::Shl,
        TwoOpOpcode::Itof => ATOO::Itof,
        TwoOpOpcode::Utof => ATOO::Utof,
        TwoOpOpcode::Ftoi => ATOO::Ftoi,
        TwoOpOpcode::Ftou => ATOO::Ftou,
        TwoOpOpcode::Ctx => ATOO::Ctx,
        #[allow(unreachable_patterns)]
        _ => unimplemented!(),
    };
    vec![AITO(
        opcode,
        vars.lookup_operand(&op1),
        vars.lookup_operand(&op2),
    )]
}

fn compile_single_op(opcode: &CSOO, op: &Operand, vars: &Vars) -> Vec<AmineInstruction> {
    use AmineInstruction::SingleOp as AISO;
    match opcode {
        SingleOpOpcode::Call => vec![AISO(ASOO::Call, vars.lookup_operand(&op))],
        SingleOpOpcode::Jmp => vec![AISO(ASOO::Jmp, vars.lookup_operand(&op))],
        SingleOpOpcode::Dbg => vec![AISO(ASOO::Dbg, vars.lookup_operand(&op))],
        #[allow(unreachable_patterns)]
        _ => unimplemented!(),
    }
}

fn compile_no_op(opcode: &CNOO, vars: &Vars) -> Vec<AmineInstruction> {
    match opcode {
        NoOpOpcode::Nop => vec![AmineInstruction::NoOp(ANOO::Nop)],
        NoOpOpcode::Ret => {
            let mut result = vars.epilogue.clone();
            result.push(AmineInstruction::NoOp(ANOO::Ret));
            result
        }
        NoOpOpcode::Send => vec![AmineInstruction::NoOp(ANOO::Send)],
        #[allow(unreachable_patterns)]
        _ => unimplemented!(),
    }
}
struct Vars {
    scopes: Vec<HashMap<String, RegOp>>,
    param_depth: usize,
    epilogue: Vec<AmineInstruction>,
}

impl Vars {
    fn new(parameters: Vec<String>, epilogue: Vec<AmineInstruction>) -> Vars {
        let parameters_len = parameters.len() as i16;
        let map = parameters
            .into_iter()
            .enumerate()
            .map(|(idx, name)| {
                (
                    name,
                    RegOp::Indirect(RawRegOp::Value((idx as i16 - 2 - parameters_len) as u16)),
                )
            })
            .collect();
        Vars {
            scopes: vec![map],
            param_depth: 0,
            epilogue,
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn push_var(&mut self, allocation: Allocation, offsets: (usize, usize, usize)) {
        let key = allocation.name;
        let value = match allocation.address {
            AbstractAddress::Volatile(idx) => {
                RegOp::Direct(RawRegOp::Register(idx_to_reg(idx).unwrap()))
            }
            AbstractAddress::RegVar(idx) => {
                RegOp::Direct(RawRegOp::Register(idx_to_reg(idx + 2).unwrap()))
            }
            AbstractAddress::StackVar(idx) => {
                RegOp::Indirect(RawRegOp::Value((idx + offsets.0) as u16))
            }
            AbstractAddress::InnerParam(idx) => {
                RegOp::Indirect(RawRegOp::Value((idx + offsets.2 + self.param_depth) as u16))
            }
            AbstractAddress::OuterResult(idx) => RegOp::Indirect(RawRegOp::Value(idx as u16)),
            _ => return,
        };
        self.scopes.last_mut().unwrap().insert(key, value);
    }

    fn lookup(&self, name: &str) -> RegOp {
        match self.scopes.iter().rev().find_map(|map| map.get(name)) {
            None => RegOp::Direct(RawRegOp::Const(name.to_string())),
            Some(reg_op) => reg_op.to_owned(),
        }
    }

    fn lookup_operand(&self, op: &chasm_ir::Operand) -> RegOp {
        match op {
            Operand::Constant(constant) => RegOp::Direct(RawRegOp::Value(constant.clone())),
            Operand::Variable(variable) => self.lookup(variable),
        }
    }
}

fn idx_to_reg(idx: usize) -> Option<Register> {
    match idx {
        0 => Some(Register::R0),
        1 => Some(Register::R1),
        2 => Some(Register::R2),
        3 => Some(Register::R3),
        4 => Some(Register::R4),
        5 => Some(Register::R5),
        6 => Some(Register::R6),
        7 => Some(Register::R7),
        _ => None,
    }
}

/// [0]: results, [n]: stack vars, [m]: stored vars, [k]: parameters
/// (reg_var_count, (n, m, k))
fn find_offsets(allocations: &[Allocation]) -> (usize, (usize, usize, usize)) {
    let (result_count, reg_var_count, stack_var_count) =
        allocations
            .iter()
            .fold((0, 0, 0), |result, allocation| match allocation.address {
                AbstractAddress::OuterParam(idx) => (result.0.max(idx + 1), result.1, result.2),
                AbstractAddress::RegVar(idx) => (result.0, result.1.max(idx + 1), result.2),
                AbstractAddress::StackVar(idx) => (result.0, result.1, result.2.max(idx + 1)),
                _ => result,
            });
    (
        reg_var_count,
        (
            result_count,
            result_count + stack_var_count,
            result_count + stack_var_count + reg_var_count,
        ),
    )
}

#[derive(Default)]
struct MemoryConfig {
    stack_vars: usize,
    reg_vars: usize,
    volatile_vars: usize,
    result_count: usize,
    param_count: usize,
}

fn determine_allocations(
    code: &[ChasmInstruction],
    cfg: &mut MemoryConfig,
    vars: &mut Vec<Allocation>,
) {
    let Some(instruction) = code.first() else {
        return;
    };

    match instruction {
        ChasmInstruction::Alloc(name, _, inner) => {
            let is_referenced = inner
                .iter_rec()
                .any(|inst| matches!(inst, ChasmInstruction::Reference(_, s) if s == name));
            let may_be_volatile = !inner.iter_rec().any(|inst| {
                matches!(
                    inst,
                    ChasmInstruction::SingleOp(chasm_ir::SingleOpOpcode::Call, _)
                )
            });
            match (is_referenced, may_be_volatile) {
                (true, _) => {
                    vars.push(Allocation {
                        name: name.clone(),
                        address: AbstractAddress::StackVar(cfg.stack_vars),
                    });
                    cfg.stack_vars += 1;
                    determine_allocations(inner, cfg, vars);
                    cfg.stack_vars -= 1;
                }
                (false, true) if cfg.volatile_vars < 2 => {
                    vars.push(Allocation {
                        name: name.clone(),
                        address: AbstractAddress::Volatile(cfg.volatile_vars),
                    });
                    cfg.volatile_vars += 1;
                    determine_allocations(inner, cfg, vars);
                    cfg.volatile_vars -= 1;
                }
                (false, _) if cfg.reg_vars < 6 => {
                    vars.push(Allocation {
                        name: name.clone(),
                        address: AbstractAddress::RegVar(cfg.reg_vars),
                    });
                    cfg.reg_vars += 1;
                    determine_allocations(inner, cfg, vars);
                    cfg.reg_vars -= 1;
                }
                (false, _) => {
                    vars.push(Allocation {
                        name: name.clone(),
                        address: AbstractAddress::StackVar(cfg.stack_vars),
                    });
                    cfg.stack_vars += 1;
                    determine_allocations(inner, cfg, vars);
                    cfg.stack_vars -= 1;
                }
            }
        }
        ChasmInstruction::Param(name, _, inner) => {
            vars.push(Allocation {
                name: name.clone(),
                address: AbstractAddress::InnerParam(cfg.result_count),
            });
            cfg.param_count += 1;
            determine_allocations(inner, cfg, vars);
            cfg.param_count -= 1;
        }
        ChasmInstruction::Result(name, _, inner) => {
            vars.push(Allocation {
                name: name.clone(),
                address: AbstractAddress::OuterResult(cfg.result_count),
            });
            cfg.result_count += 1;
            determine_allocations(inner, cfg, vars);
            cfg.result_count -= 1;
        }
        _ => {}
    }

    determine_allocations(&code[1..], cfg, vars);
}
