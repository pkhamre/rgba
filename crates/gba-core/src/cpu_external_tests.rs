//! Optional reference-emulator differential checks, not real-hardware timing tests.

use super::Cpu;
use crate::{AccessKind, AccessWidth, CpuBus, EmulatorError, ProcessorStatus};
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug)]
struct Transaction {
    kind: u32,
    size: u32,
    address: u32,
    data: u32,
    access: u32,
}

struct VectorBus {
    transactions: Vec<Transaction>,
    index: usize,
    mismatch: Option<String>,
    clocks: u64,
}

impl VectorBus {
    fn transaction(
        &mut self,
        kind: u32,
        address: u32,
        width: AccessWidth,
        access: AccessKind,
        data: Option<u32>,
    ) -> u32 {
        let data = data.map(|value| match width {
            AccessWidth::Byte => value & 0xff,
            AccessWidth::Halfword => value & 0xffff,
            AccessWidth::Word => value,
        });
        let size = match width {
            AccessWidth::Byte => 1,
            AccessWidth::Halfword => 2,
            AccessWidth::Word => 4,
        };
        let Some(expected) = self.transactions.get(self.index) else {
            self.mismatch
                .get_or_insert_with(|| format!("extra transaction {kind} {address:08x}"));
            return 0;
        };
        self.index += 1;
        if expected.kind != kind
            || expected.address != address
            || expected.size != size
            || (expected.access & 1 != 0) != (access == AccessKind::Sequential)
            || data.is_some_and(|value| value != expected.data)
        {
            self.mismatch.get_or_insert_with(|| format!(
                "transaction {} expected {expected:?}, got kind={kind} address={address:08x} size={size} access={access:?} data={data:?}",
                self.index - 1));
        }
        expected.data
    }
}

impl CpuBus for VectorBus {
    fn clocks(&self) -> u64 {
        self.clocks
    }
    fn internal_cycles(&mut self, count: u64) {
        self.clocks += count;
    }
    fn set_execution_address(&mut self, _address: u32) {}
    fn fetch(
        &mut self,
        address: u32,
        width: AccessWidth,
        kind: AccessKind,
    ) -> Result<u32, EmulatorError> {
        Ok(self.transaction(0, address, width, kind, None))
    }
    fn read(
        &mut self,
        address: u32,
        width: AccessWidth,
        kind: AccessKind,
    ) -> Result<u32, EmulatorError> {
        Ok(self.transaction(1, address, width, kind, None))
    }
    fn write(
        &mut self,
        address: u32,
        width: AccessWidth,
        value: u32,
        kind: AccessKind,
    ) -> Result<(), EmulatorError> {
        self.transaction(2, address, width, kind, Some(value));
        Ok(())
    }
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("vector contains whole words"),
    )
}

fn state(bytes: &[u8], offset: usize) -> [u32; 40] {
    let mut result = [0; 40];
    for (index, value) in result.iter_mut().enumerate() {
        *value = word(bytes, offset + 8 + index * 4);
    }
    result
}

fn cpu_from_state(initial: &[u32; 40]) -> Cpu {
    let status = ProcessorStatus::try_new(initial[31]).expect("vector has valid processor mode");
    let mut cpu = Cpu {
        address: initial[15].wrapping_sub(if status.thumb() { 4 } else { 8 }),
        status: ProcessorStatus::try_new((initial[31] & !0x1f) | 0x10).expect("User mode"),
        pipeline: Some([initial[37], initial[38]]),
        next_fetch: if initial[39] & 1 != 0 {
            AccessKind::Sequential
        } else {
            AccessKind::Nonsequential
        },
        ..Cpu::default()
    };
    cpu.registers.copy_from_slice(&initial[..15]);
    cpu.normal_high.copy_from_slice(&initial[8..13]);
    cpu.fiq_high.copy_from_slice(&initial[16..21]);
    for (bank, offset) in [(1, 21), (3, 23), (4, 25), (2, 27), (5, 29)] {
        cpu.banks[bank].copy_from_slice(&initial[offset..offset + 2]);
    }
    // The vector R array is the User bank, not the currently visible registers.
    cpu.replace_status(status);
    for (bank, offset) in [(1, 32), (3, 33), (4, 34), (2, 35), (5, 36)] {
        cpu.saved_status[bank] =
            ProcessorStatus::try_new(initial[offset]).expect("saved status has a valid mode");
    }
    cpu
}

fn run_vector(bytes: &[u8], start: usize) -> Result<(), String> {
    let initial_offset = start + 4;
    let final_offset = initial_offset + word(bytes, initial_offset) as usize;
    let transactions_offset = final_offset + word(bytes, final_offset) as usize;
    let opcode_offset = transactions_offset + word(bytes, transactions_offset) as usize;
    let opcode = word(bytes, opcode_offset + 8);
    let initial = state(bytes, initial_offset);
    let expected = state(bytes, final_offset);
    let mut cpu = cpu_from_state(&initial);
    let count = word(bytes, transactions_offset + 8) as usize;
    let mut transactions = Vec::with_capacity(count);
    for index in 0..count {
        let offset = transactions_offset + 12 + index * 24;
        transactions.push(Transaction {
            kind: word(bytes, offset),
            size: word(bytes, offset + 4),
            address: word(bytes, offset + 8),
            data: word(bytes, offset + 12),
            access: word(bytes, offset + 20),
        });
    }
    let mut bus = VectorBus {
        transactions,
        index: 0,
        mismatch: None,
        clocks: 0,
    };
    cpu.step(&mut bus).map_err(|error| error.to_string())?;
    if let Some(mismatch) = bus.mismatch {
        return Err(format!("opcode {opcode:08x}: {mismatch}"));
    }
    if bus.index != bus.transactions.len() {
        return Err(format!("opcode {opcode:08x}: missing transactions"));
    }
    let mut actual = [0; 16];
    for (index, value) in actual.iter_mut().enumerate() {
        *value = cpu.user_register(index);
    }
    if actual != expected[..16] {
        return Err(format!(
            "opcode {opcode:08x} initial CPSR={:08x}: registers expected {:08x?}, got {:08x?}",
            initial[31],
            &expected[..16],
            actual
        ));
    }
    if cpu.status.bits() != expected[31] {
        return Err(format!(
            "opcode {opcode:08x}: CPSR expected {:08x}, got {:08x}",
            expected[31],
            cpu.status.bits()
        ));
    }
    let active_bank = cpu.status.mode().bank();
    for (bank, offset) in [(1, 21), (3, 23), (4, 25), (2, 27), (5, 29)] {
        let actual = if active_bank == bank {
            [cpu.registers[13], cpu.registers[14]]
        } else {
            cpu.banks[bank]
        };
        if actual != expected[offset..offset + 2] {
            return Err(format!("opcode {opcode:08x}: bank {bank} SP/LR differs"));
        }
    }
    let high = if active_bank == 1 {
        &cpu.registers[8..13]
    } else {
        &cpu.fiq_high
    };
    if high != &expected[16..21] {
        return Err(format!("opcode {opcode:08x}: FIQ R8–R12 differs"));
    }
    for (bank, offset) in [(1, 32), (3, 33), (4, 34), (2, 35), (5, 36)] {
        if cpu.saved_status[bank].bits() != expected[offset] {
            return Err(format!("opcode {opcode:08x}: bank {bank} SPSR differs"));
        }
    }
    if (cpu.next_fetch == AccessKind::Sequential) != (expected[39] & 1 != 0) {
        return Err(format!(
            "opcode {opcode:08x}: next fetch access kind differs"
        ));
    }
    if cpu.pipeline != Some([expected[37], expected[38]]) {
        return Err(format!("opcode {opcode:08x}: pipeline differs"));
    }
    Ok(())
}

#[test]
fn vector_bus_checks_sequential_access_flags() {
    let mut bus = VectorBus {
        transactions: vec![Transaction {
            kind: 0,
            size: 4,
            address: 0x0300_0000,
            data: 0x1234,
            access: 3,
        }],
        index: 0,
        mismatch: None,
        clocks: 0,
    };
    let value = bus
        .fetch(0x0300_0000, AccessWidth::Word, AccessKind::Nonsequential)
        .expect("vector bus still returns expected data for diagnostics");
    assert_eq!(value, 0x1234);
    assert!(
        bus.mismatch.is_some(),
        "a wrong N/S flag must not pass silently"
    );
}

#[test]
fn vector_initial_access_flags_seed_the_cpu_fetch_stream() {
    let mut state = [0; 40];
    state[31] = 0x1f;
    state[32..37].fill(0x13);
    state[15] = 8;
    for (access, expected) in [(2, AccessKind::Nonsequential), (3, AccessKind::Sequential)] {
        state[39] = access;
        let cpu = cpu_from_state(&state);
        assert_eq!(cpu.next_fetch, expected);
    }
}

#[test]
#[ignore = "fetch reference vectors with python3 scripts/fetch_cpu_vectors.py; see docs/TESTING.md"]
fn external_cpu_vectors() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets/arm7tdmi/v1");
    use std::io::Write;

    let filter = env::var("GBA_VECTOR_FILTER").unwrap_or_default();
    let selected_index: Option<u32> = env::var("GBA_VECTOR_INDEX").ok().map(|value| {
        value
            .parse()
            .expect("GBA_VECTOR_INDEX must be a nonnegative 32-bit integer")
    });
    let mut report = env::var_os("GBA_VECTOR_REPORT").map(|path| {
        let path = PathBuf::from(path);
        let path = if path.is_absolute() {
            path
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(path)
        };
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("GBA_VECTOR_REPORT must name a new writable file")
    });
    if let Some(report) = &mut report {
        writeln!(report, "group\tindex\tdetail").expect("report header is writable");
    }
    let mut files: Vec<_> = fs::read_dir(root)
        .expect("reference vectors are downloaded")
        .map(|entry| entry.expect("readable vector directory").path())
        .filter(|path| {
            path.to_string_lossy().ends_with(".json.bin")
                && path.to_string_lossy().contains(&filter)
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no vector groups match {filter:?}");
    if selected_index.is_some() {
        assert_eq!(
            files.len(),
            1,
            "single-case replay requires a filter matching exactly one group"
        );
    }
    let mut total_passed = 0;
    let mut total_failed = 0;
    for file in files {
        let bytes = fs::read(&file).expect("vector file is readable");
        assert_eq!(word(&bytes, 0), 0xd33d_bae0, "vector format magic");
        let count = word(&bytes, 4);
        let mut offset = 8;
        if let Some(index) = selected_index {
            assert!(index < count, "vector index {index} is outside 0..{count}");
        }
        let group = file.file_name().expect("file has name").to_string_lossy();
        let mut failures = 0;
        let mut tested = 0;
        let mut first_failure = None;
        for index in 0..count {
            if selected_index.is_none_or(|selected| selected == index) {
                tested += 1;
                if let Err(message) = run_vector(&bytes, offset) {
                    failures += 1;
                    if let Some(report) = &mut report {
                        writeln!(report, "{group}\t{index}\t{message}")
                            .expect("failure report is writable");
                    }
                    first_failure.get_or_insert((index, message));
                }
            }
            offset += word(&bytes, offset) as usize;
        }
        assert_eq!(offset, bytes.len(), "consume complete vector file");
        eprintln!(
            "{}: {}/{} pass; first failure: {first_failure:?}",
            group,
            tested - failures,
            tested
        );
        total_passed += tested - failures;
        total_failed += failures;
    }
    eprintln!("total: {total_passed} pass, {total_failed} fail");
    assert_eq!(
        total_failed, 0,
        "reference mismatches are not silently skipped"
    );
}
