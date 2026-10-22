//! Isolated mGBA suite CPU-table replay, not execution of the complete test ROM.

use super::Cpu;
use crate::{Bus, ProcessorStatus};
use std::env;
use std::fs;
use std::path::PathBuf;

const INSTRUCTION_ADDRESS: u32 = 0x0800_0100;
const SCHEMA: &str = "gba-cpu-cases-v1";
const COLUMNS: &str =
    "suite\tindex\tname\topcode\tin_cpsr\tin_r0\tin_r1\tout_reg0\tout0\tout_reg1\tout1\tout_cpsr";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaseGroup {
    Carry,
    Shifter,
    MultiplyLong,
}

impl CaseGroup {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "carry" => Ok(Self::Carry),
            "shifter" => Ok(Self::Shifter),
            "multiply-long" => Ok(Self::MultiplyLong),
            _ => Err(format!("unknown CPU baseline group {value:?}")),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Carry => "carry",
            Self::Shifter => "shifter",
            Self::MultiplyLong => "multiply-long",
        }
    }

    fn slot(self) -> usize {
        match self {
            Self::Carry => 0,
            Self::Shifter => 1,
            Self::MultiplyLong => 2,
        }
    }

    fn count(self) -> usize {
        match self {
            Self::Carry => 93,
            Self::Shifter => 70,
            Self::MultiplyLong => 72,
        }
    }
}

#[derive(Clone, Debug)]
struct CpuCase {
    group: CaseGroup,
    index: usize,
    name: String,
    opcode: u32,
    in_cpsr: u32,
    in_r0: u32,
    in_r1: u32,
    out_reg0: usize,
    out0: u32,
    out_reg1: Option<usize>,
    out1: Option<u32>,
    out_cpsr: u32,
}

fn hex_word(value: &str) -> Result<u32, String> {
    if value.len() != 8 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("expected eight hexadecimal digits, got {value:?}"));
    }
    u32::from_str_radix(value, 16).map_err(|error| error.to_string())
}

fn output_register(value: &str) -> Result<usize, String> {
    let index: usize = value
        .parse()
        .map_err(|_| format!("invalid register {value:?}"))?;
    if index >= 15 || index == 12 {
        return Err(format!(
            "invalid output register r{index}: PC and CPSR capture are reserved"
        ));
    }
    Ok(index)
}

fn parse_case(line: &str) -> Result<CpuCase, String> {
    let fields: Vec<_> = line.split('\t').collect();
    if fields.len() != 12 {
        return Err(format!("expected 12 columns, got {}", fields.len()));
    }
    let group = CaseGroup::parse(fields[0])?;
    let index: usize = fields[1]
        .parse()
        .map_err(|_| "invalid case index".to_owned())?;
    if index >= group.count() || fields[2].is_empty() || fields[2].contains('\r') {
        return Err("invalid case index or name".to_owned());
    }
    let out_reg0 = output_register(fields[7])?;
    let (out_reg1, out1) = match (fields[9], fields[10]) {
        ("-", "-") => (None, None),
        ("-", _) | (_, "-") => return Err("incomplete second output pair".to_owned()),
        (register, value) => (Some(output_register(register)?), Some(hex_word(value)?)),
    };
    let expected_registers = match group {
        CaseGroup::Carry => (2, None),
        CaseGroup::Shifter => (0, None),
        CaseGroup::MultiplyLong => (3, Some(2)),
    };
    if (out_reg0, out_reg1) != expected_registers {
        return Err("output registers do not match the pinned lowering protocol".to_owned());
    }
    Ok(CpuCase {
        group,
        index,
        name: fields[2].to_owned(),
        opcode: hex_word(fields[3])?,
        in_cpsr: hex_word(fields[4])?,
        in_r0: hex_word(fields[5])?,
        in_r1: hex_word(fields[6])?,
        out_reg0,
        out0: hex_word(fields[8])?,
        out_reg1,
        out1,
        out_cpsr: hex_word(fields[11])?,
    })
}

fn parse_cases(contents: &str) -> Result<Vec<CpuCase>, String> {
    let mut lines = contents.lines();
    if lines.next() != Some(SCHEMA) || lines.next() != Some(COLUMNS) {
        return Err("CPU baseline schema/header mismatch".to_owned());
    }
    let mut seen = [vec![false; 93], vec![false; 70], vec![false; 72]];
    let mut cases = Vec::with_capacity(235);
    for (index, line) in lines.enumerate() {
        let case = parse_case(line).map_err(|error| format!("line {}: {error}", index + 3))?;
        let entry = &mut seen[case.group.slot()][case.index];
        if *entry {
            return Err(format!(
                "duplicate {} case {}",
                case.group.name(),
                case.index
            ));
        }
        *entry = true;
        cases.push(case);
    }
    if seen.iter().any(|group| group.contains(&false)) {
        return Err(
            "missing pinned cases: require carry=93, shifter=70, multiply-long=72".to_owned(),
        );
    }
    Ok(cases)
}

fn run_case(case: &CpuCase) -> Result<(), String> {
    // Private test-only entry setup. Machine::try_new still always resets into
    // the supplied BIOS; this does not expose a skip-BIOS game execution path.
    let mut rom = [0; 0x200];
    let program = [0xe128_f00c, case.opcode, 0xe10f_c000]; // MSR flags; test; MRS.
    for (index, opcode) in program.into_iter().enumerate() {
        let offset = 0xfc + index * 4;
        rom[offset..offset + 4].copy_from_slice(&opcode.to_le_bytes());
    }
    let mut bus = Bus::try_new(&[0; 0x4000], &rom).map_err(|error| error.to_string())?;
    let mut cpu = Cpu::default();
    cpu.replace_status(ProcessorStatus::try_new(0x1f).expect("ARM System mode is valid"));
    cpu.registers[0] = case.in_r0;
    cpu.registers[1] = case.in_r1;
    cpu.registers[12] = case.in_cpsr;
    cpu.branch(INSTRUCTION_ADDRESS - 4);
    for _ in 0..3 {
        cpu.step(&mut bus).map_err(|error| error.to_string())?;
    }
    if cpu.instruction_address() != INSTRUCTION_ADDRESS + 8 {
        return Err(format!(
            "program did not finish at 08000108: PC={:08x}",
            cpu.instruction_address()
        ));
    }
    let mut differences = Vec::new();
    if cpu.registers[case.out_reg0] != case.out0 {
        differences.push(format!(
            "r{} expected {:08x}, got {:08x}",
            case.out_reg0, case.out0, cpu.registers[case.out_reg0]
        ));
    }
    if let (Some(register), Some(expected)) = (case.out_reg1, case.out1)
        && cpu.registers[register] != expected
    {
        differences.push(format!(
            "r{register} expected {expected:08x}, got {:08x}",
            cpu.registers[register]
        ));
    }
    if cpu.registers[12] != case.out_cpsr {
        differences.push(format!(
            "CPSR expected {:08x}, got {:08x}",
            case.out_cpsr, cpu.registers[12]
        ));
    }
    if cpu.registers[12] != cpu.status().bits() {
        differences.push("MRS result does not equal the current CPSR".to_owned());
    }
    if differences.is_empty() {
        Ok(())
    } else {
        Err(differences.join("; "))
    }
}

#[test]
#[ignore = "fetch independent CPU baselines with python3 scripts/fetch_mgba_cpu_cases.py"]
fn mgba_cpu_baselines() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/assets/mgba-suite-cpu/cases.tsv");
    let contents = fs::read_to_string(path).expect("fetch mGBA CPU baseline data first");
    let cases = parse_cases(&contents).expect("complete, well-formed pinned cases");
    let selected_group = env::var("GBA_SUITE_GROUP").ok().map(|value| {
        CaseGroup::parse(&value).expect("GBA_SUITE_GROUP must be carry, shifter, or multiply-long")
    });
    let selected_index: Option<usize> = env::var("GBA_SUITE_CASE_INDEX").ok().map(|value| {
        value
            .parse()
            .expect("GBA_SUITE_CASE_INDEX must be a nonnegative integer")
    });
    if let Some(index) = selected_index {
        let group = selected_group.expect("case-index replay requires GBA_SUITE_GROUP");
        assert!(
            index < group.count(),
            "case index must be less than {}",
            group.count()
        );
    }
    let mut counts = [(0, 0); 3]; // Completed and failed cases for each group.
    for case in &cases {
        if selected_group.is_some_and(|group| group != case.group)
            || selected_index.is_some_and(|index| index != case.index)
        {
            continue;
        }
        counts[case.group.slot()].0 += 1;
        if let Err(error) = run_case(case) {
            counts[case.group.slot()].1 += 1;
            eprintln!(
                "FAIL {} case {} {:?}, opcode {:08x}: {error}",
                case.group.name(),
                case.index,
                case.name,
                case.opcode
            );
        }
    }
    let mut total = 0;
    let mut failed = 0;
    for group in [
        CaseGroup::Carry,
        CaseGroup::Shifter,
        CaseGroup::MultiplyLong,
    ] {
        let (completed, failures) = counts[group.slot()];
        if completed != 0 {
            eprintln!(
                "{}: {}/{} isolated instruction cases pass",
                group.name(),
                completed - failures,
                completed
            );
        }
        total += completed;
        failed += failures;
    }
    assert!(total > 0, "baseline selection must execute cases");
    eprintln!(
        "mGBA CPU baselines: {} pass, {failed} fail out of {total}",
        total - failed
    );
    assert_eq!(
        failed, 0,
        "upstream expected results must match without masking flags"
    );
}

fn synthetic_case() -> CpuCase {
    parse_case("carry\t0\tsynthetic ADCS\te0b02001\t2000001f\t00000001\t00000002\t2\t00000004\t-\t-\t0000001f")
        .expect("valid synthetic case")
}

#[test]
fn synthetic_replay_checks_result_and_full_status() {
    let case = synthetic_case();
    run_case(&case).expect("1 + 2 + C equals 4");
    let mut wrong_result = case.clone();
    wrong_result.out0 = 9;
    assert!(
        run_case(&wrong_result)
            .expect_err("must reject wrong output")
            .contains("r2")
    );
    let mut wrong_status = case;
    wrong_status.out_cpsr = 0x2000_001f;
    assert!(
        run_case(&wrong_status)
            .expect_err("must not mask carry")
            .contains("CPSR")
    );
}

#[test]
fn synthetic_replay_bounds_a_loop_and_rejects_exception_entry() {
    let mut case = synthetic_case();
    case.opcode = 0xeaff_fffe; // B .
    assert!(
        run_case(&case)
            .expect_err("three-step budget must stop a loop")
            .contains("did not finish")
    );
    case.opcode = 0xee10_0f10; // Absent coprocessor enters Undefined mode.
    assert!(
        run_case(&case)
            .expect_err("exception cannot be a test pass")
            .contains("did not finish")
    );
}

#[test]
fn malformed_baseline_rows_fail_explicitly() {
    let valid =
        "carry\t0\texample\te0b02001\t0000001f\t00000000\t00000000\t2\t00000000\t-\t-\t4000001f";
    for row in [
        "",
        "carry\t0",
        &valid.replace("carry", "unknown"),
        &valid.replacen("\t0\t", "\t93\t", 1),
        &valid.replace("e0b02001", "0xe0b02001"),
        &valid.replace("e0b02001", "zzzzzzzz"),
        &valid.replace("\t2\t00000000", "\t15\t00000000"),
        &valid.replace("\t2\t00000000", "\t12\t00000000"),
        &valid.replace("\t-\t-\t", "\t-\t00000000\t"),
        &valid.replace("carry", "multiply-long"),
    ] {
        assert!(
            parse_case(row).is_err(),
            "must reject malformed row {row:?}"
        );
    }
}

#[test]
fn full_baseline_parser_rejects_missing_or_duplicate_cases() {
    let row =
        "carry\t0\texample\te0b02001\t0000001f\t00000000\t00000000\t2\t00000000\t-\t-\t4000001f";
    let partial = format!("{SCHEMA}\n{COLUMNS}\n{row}\n");
    assert!(
        parse_cases(&partial)
            .expect_err("cannot silently reduce coverage")
            .contains("missing")
    );
    let duplicate = format!("{partial}{row}\n");
    assert!(
        parse_cases(&duplicate)
            .expect_err("duplicates cannot inflate coverage")
            .contains("duplicate")
    );
    assert!(parse_cases(&partial.replace(SCHEMA, "gba-cpu-cases-v2")).is_err());
}
