//! Synthetic instruction-level regressions. These do not establish hardware accuracy.

use super::Cpu;
use crate::alu;
use crate::{AccessKind, AccessWidth, Bus, ProcessorMode, ProcessorStatus};

const START: u32 = 0x0300_0000;
const DATA: u32 = 0x0200_0100;

fn fixture(opcodes: &[u32], thumb: bool) -> (Cpu, Bus) {
    let mut bus = Bus::try_new(&[0; 0x4000], &[0; 192]).expect("valid test images");
    let width = if thumb {
        AccessWidth::Halfword
    } else {
        AccessWidth::Word
    };
    let stride = if thumb { 2 } else { 4 };
    for (index, opcode) in opcodes.iter().enumerate() {
        bus.write(
            START + index as u32 * stride,
            width,
            *opcode,
            AccessKind::Nonsequential,
        )
        .expect("RAM accepts code");
    }
    let mut cpu = Cpu::default();
    cpu.replace_status(
        ProcessorStatus::try_new(if thumb { 0x3f } else { 0x1f }).expect("System mode"),
    );
    cpu.branch(START);
    (cpu, bus)
}

fn run(cpu: &mut Cpu, bus: &mut Bus, steps: usize) {
    for _ in 0..steps {
        cpu.step(bus).expect("supported test instruction");
    }
}

fn write(bus: &mut Bus, address: u32, value: u32) {
    bus.write(address, AccessWidth::Word, value, AccessKind::Nonsequential)
        .expect("test RAM write");
}

fn read(bus: &mut Bus, address: u32) -> u32 {
    bus.read(address, AccessWidth::Word, AccessKind::Nonsequential)
        .expect("test RAM read")
}

#[test]
fn legacy_teqp_restores_saved_status_without_branching() {
    // ARM7TDMI data sheet 4.5.6: TEQP copies SPSR to CPSR in privileged modes.
    let (mut cpu, mut bus) = fixture(&[0xe130_f001], false);
    cpu.replace_status(ProcessorStatus::try_new(0x13).expect("Supervisor mode"));
    cpu.saved_status[3] = ProcessorStatus::try_new(0x6000_0092).expect("saved IRQ status");
    cpu.registers[0] = 0x8000_0000;
    run(&mut cpu, &mut bus, 1);
    assert_eq!(
        cpu.status.bits(),
        0x6000_0092,
        "TEQP must not use ordinary TEQ flags"
    );
    assert_eq!(cpu.address, START + 4, "TEQP does not write the PC");
}

#[test]
fn legacy_teqp_does_not_change_user_flags() {
    let (mut cpu, mut bus) = fixture(&[0xe130_f001], false);
    cpu.replace_status(ProcessorStatus::try_new(0x9000_0010).expect("User mode"));
    run(&mut cpu, &mut bus, 1);
    assert_eq!(
        cpu.status.bits(),
        0x9000_0010,
        "User has no SPSR to restore"
    );
    assert_eq!(cpu.address, START + 4);
}

#[test]
fn thumb_multiply_reconstructs_carry_and_preserves_overflow() {
    // Minimized SingleStepTests thumb_data_proc case 165; see CPU_RESEARCH.md.
    let (mut cpu, mut bus) = fixture(&[0x4342], true); // MULS r2, r0.
    cpu.registers[0] = 0x85ea_0fec;
    cpu.registers[2] = 0xbce4_cb58;
    cpu.status.set_overflow(true);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[2], 0x2145_9d20);
    assert!(
        cpu.status.carry(),
        "this product does not have constant-zero carry"
    );
    assert!(cpu.status.overflow(), "multiply preserves V");
}

#[test]
fn absent_coprocessor_uses_the_undefined_exception_vector() {
    let (mut cpu, mut bus) = fixture(&[0xee10_0f10], false);
    let old_status = cpu.status;
    let step = cpu
        .step(&mut bus)
        .expect("absent coprocessor traps, not host failure");
    assert_eq!(step.address, START);
    assert_eq!(cpu.address, 4);
    assert_eq!(cpu.status.mode(), ProcessorMode::Undefined);
    assert_eq!(cpu.registers[14], START + 4);
    assert_eq!(cpu.saved_status[5], old_status);
    assert!(cpu.status.irq_disabled());
}

#[test]
fn thumb_undefined_encoding_saves_thumb_return_state() {
    let (mut cpu, mut bus) = fixture(&[0xde00], true);
    let old_status = cpu.status;
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.address, 4);
    assert_eq!(cpu.status.mode(), ProcessorMode::Undefined);
    assert!(!cpu.status.thumb());
    assert_eq!(cpu.registers[14], START + 2);
    assert_eq!(cpu.saved_status[5], old_status);
}

#[test]
fn internal_cycles_break_the_next_instruction_fetch_stream() {
    // GBATEK GamePak Prefetch: with prefetch disabled, I-cycle instructions
    // such as register shifts and multiply force the next code access to N.
    for (opcode, thumb) in [
        (0xe1a0_0211, false), // MOV r0, r1, LSL r2.
        (0xe000_0291, false), // MUL r0, r1, r2.
        (0xe091_0392, false), // UMULLS r0, r1, r2, r3.
        (0x4088, true),       // LSL r0, r1.
        (0x4348, true),       // MUL r0, r1.
    ] {
        let (mut cpu, mut bus) = fixture(&[opcode], thumb);
        run(&mut cpu, &mut bus, 1);
        assert_eq!(
            cpu.next_fetch,
            AccessKind::Nonsequential,
            "opcode {opcode:08x}"
        );
    }
}

#[test]
fn failed_condition_does_not_create_multiply_internal_cycles() {
    let (mut cpu, mut bus) = fixture(&[0x0000_0291], false); // MULEQ, Z=0.
    let trace = cpu.step(&mut bus).expect("failed condition skips multiply");
    assert!(!trace.condition_passed);
    assert_eq!(cpu.next_fetch, AccessKind::Sequential);
    assert_eq!(
        trace.clock_after - trace.clock_before,
        3,
        "startup pipeline fill only"
    );
}

#[test]
fn internal_cycle_instruction_charges_a_nonsequential_rom_fetch_next() {
    for opcode in [0xe1a0_0211u32, 0xe000_0291, 0xe091_0392] {
        let mut rom = [0; 192];
        for (index, opcode) in [opcode, 0xe1a0_0000, 0xe1a0_0000, 0xe1a0_0000]
            .into_iter()
            .enumerate()
        {
            rom[index * 4..index * 4 + 4].copy_from_slice(&opcode.to_le_bytes());
        }
        let mut bus = Bus::try_new(&[0; 0x4000], &rom).expect("synthetic images");
        let mut cpu = Cpu::default();
        cpu.branch(0x0800_0000);
        run(&mut cpu, &mut bus, 1);
        let after_internal = cpu.step(&mut bus).expect("NOP after I-cycle instruction");
        let sequential = cpu.step(&mut bus).expect("second NOP");
        assert_eq!(
            after_internal.clock_after - after_internal.clock_before,
            8,
            "default ROM word N access after {opcode:08x}"
        );
        assert_eq!(
            sequential.clock_after - sequential.clock_before,
            6,
            "NOP resumes default ROM word S accesses"
        );
    }
}

#[test]
fn non_flag_multiply_preserves_all_status_flags() {
    for opcode in [0xe000_0291, 0xe081_0392] {
        let (mut cpu, mut bus) = fixture(&[opcode], false);
        cpu.replace_status(ProcessorStatus::try_new(0xf000_001f).expect("System mode and NZCV"));
        cpu.registers[1..4].copy_from_slice(&[2, 3, 4]);
        run(&mut cpu, &mut bus, 1);
        assert_eq!(cpu.status.bits(), 0xf000_001f, "S=0 opcode {opcode:08x}");
    }
}

#[test]
fn undefined_handler_can_restore_thumb_execution() {
    let mut bios = [0; 0x4000];
    bios[4..8].copy_from_slice(&0xe1b0_f00eu32.to_le_bytes()); // MOVS pc, lr.
    let mut bus = Bus::try_new(&bios, &[0; 192]).expect("synthetic handler BIOS");
    bus.write(
        START,
        AccessWidth::Halfword,
        0xde00,
        AccessKind::Nonsequential,
    )
    .expect("undefined test opcode");
    bus.write(
        START + 2,
        AccessWidth::Halfword,
        0x202a,
        AccessKind::Nonsequential,
    )
    .expect("MOV r0, #42");
    let (mut cpu, _) = fixture(&[], true);
    cpu.registers[13] = DATA;
    run(&mut cpu, &mut bus, 2);
    assert_eq!(cpu.status.mode(), ProcessorMode::System);
    assert!(cpu.status.thumb());
    assert_eq!(
        cpu.registers[13], DATA,
        "exception return restores the original register bank"
    );
    assert_eq!(cpu.address, START + 2);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[0], 42);
}

#[test]
fn reset_and_architectural_pc() {
    let cpu = Cpu::default();
    assert_eq!(cpu.instruction_address(), 0);
    assert_eq!(cpu.registers()[15], 8);
    assert_eq!(cpu.status().bits(), 0xd3);
    let (cpu, _) = fixture(&[], true);
    assert_eq!(cpu.registers()[15], START + 4);
}

#[test]
fn arm_conditional_loop() {
    // MOV r0, #3; SUBS r0, r0, #1; BNE -12; MOVEQ r1, #42; MOVNE r1, #99.
    let (mut cpu, mut bus) = fixture(
        &[
            0xe3a0_0003,
            0xe250_0001,
            0x1aff_fffd,
            0x03a0_102a,
            0x13a0_1063,
        ],
        false,
    );
    run(&mut cpu, &mut bus, 8);
    let skipped = cpu.step(&mut bus).expect("condition-failed instruction");
    assert!(!skipped.condition_passed);
    assert_eq!(cpu.registers[0], 0);
    assert_eq!(cpu.registers[1], 42);
}

#[test]
fn arm_bl_and_bx_return() {
    let (mut cpu, mut bus) = fixture(&[0xeb00_0000, 0xe3a0_0007, 0xe12f_ff1e], false);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[14], START + 4);
    assert_eq!(cpu.address, START + 8);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.address, START + 4);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[0], 7);
}

#[test]
fn exchange_selects_state_and_aligns_address() {
    let (mut cpu, mut bus) = fixture(&[0xe12f_ff10, 0, 0x2109_2005], false);
    cpu.registers[0] = START + 9;
    run(&mut cpu, &mut bus, 1);
    assert!(cpu.status.thumb());
    assert_eq!(cpu.address, START + 8);
    run(&mut cpu, &mut bus, 2);
    assert_eq!(&cpu.registers[..2], &[5, 9]);
}

#[test]
fn arm_logical_operations_and_flags() {
    for (operation, expected, test_only) in [
        (0, 0x00f0, false),
        (1, 0xff00, false),
        (8, 0x00f0, true),
        (9, 0xff00, true),
        (12, 0xfff0, false),
        (13, 0x0ff0, false),
        (14, 0xf000, false),
        (15, !0x0ff0u32, false),
    ] {
        let opcode = 0xe010_0000 | (operation << 21) | (1 << 16) | 2;
        let (mut cpu, mut bus) = fixture(&[opcode], false);
        cpu.registers[0] = 0xdead_beef;
        cpu.registers[1] = 0xf0f0;
        cpu.registers[2] = 0x0ff0;
        cpu.status.set_carry(true);
        cpu.status.set_overflow(true);
        run(&mut cpu, &mut bus, 1);
        assert_eq!(
            cpu.registers[0],
            if test_only { 0xdead_beef } else { expected },
            "operation {operation}"
        );
        assert_eq!(cpu.status.negative(), expected >> 31 != 0);
        assert_eq!(cpu.status.zero(), expected == 0);
        assert!(cpu.status.carry(), "unshifted operand preserves carry");
        assert!(cpu.status.overflow(), "logical operations preserve V");
    }
}

#[test]
fn arm_arithmetic_operations_follow_carry_oracle() {
    for (left, right) in [(0, 1), (u32::MAX, 1), (0x7fff_ffff, 1), (0x8000_0000, 1)] {
        for carry in [false, true] {
            for operation in [2, 3, 4, 5, 6, 7, 10, 11] {
                let expected = match operation {
                    2 | 10 => alu::add_with_carry(left, !right, true),
                    3 => alu::add_with_carry(right, !left, true),
                    4 | 11 => alu::add_with_carry(left, right, false),
                    5 => alu::add_with_carry(left, right, carry),
                    6 => alu::add_with_carry(left, !right, carry),
                    7 => alu::add_with_carry(right, !left, carry),
                    _ => unreachable!(),
                };
                let opcode = 0xe010_0000 | (operation << 21) | (1 << 16) | 2;
                let (mut cpu, mut bus) = fixture(&[opcode], false);
                cpu.registers[0] = 0xdead_beef;
                cpu.registers[1] = left;
                cpu.registers[2] = right;
                cpu.status.set_carry(carry);
                run(&mut cpu, &mut bus, 1);
                assert_eq!(
                    cpu.registers[0],
                    if operation >= 10 {
                        0xdead_beef
                    } else {
                        expected.value
                    }
                );
                assert_eq!(cpu.status.carry(), expected.carry, "operation {operation}");
                assert_eq!(
                    cpu.status.overflow(),
                    expected.overflow,
                    "operation {operation}"
                );
                assert_eq!(cpu.status.zero(), expected.value == 0);
                assert_eq!(cpu.status.negative(), expected.value >> 31 != 0);
            }
        }
    }
}

#[test]
fn arm_rotated_immediate_updates_shifter_carry() {
    // MOVS r0, #0x80000000; MOVS r1, #0.
    let (mut cpu, mut bus) = fixture(&[0xe3b0_0102, 0xe3b0_1000], false);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[0], 0x8000_0000);
    assert!(cpu.status.carry());
    run(&mut cpu, &mut bus, 1);
    assert!(cpu.status.carry(), "zero rotation preserves C");
    assert!(cpu.status.zero());
}

#[test]
fn register_shifted_pc_reads_plus_twelve() {
    // ADD r0, pc, pc, LSL r1.
    let (mut cpu, mut bus) = fixture(&[0xe08f_011f], false);
    cpu.registers[1] = 0;
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[0], (START + 12) * 2);
}

#[test]
fn register_shift_uses_only_low_byte() {
    // MOVS r0, r1, LSL r2.
    let (mut cpu, mut bus) = fixture(&[0xe1b0_0211], false);
    cpu.registers[1] = 0x1234;
    cpu.registers[2] = 0x100;
    cpu.status.set_carry(true);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[0], 0x1234);
    assert!(cpu.status.carry());
}

#[test]
fn arm_store_pc_and_load_pc_do_not_exchange_state() {
    let (mut cpu, mut bus) = fixture(&[0xe580_f000, 0xe590_f000], false);
    cpu.registers[0] = DATA;
    run(&mut cpu, &mut bus, 1);
    assert_eq!(read(&mut bus, DATA), START + 12);
    write(&mut bus, DATA, START + 19);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.address, START + 16);
    assert!(!cpu.status.thumb());
}

#[test]
fn arm_ldr_rotates_misaligned_words_and_str_aligns() {
    for offset in 0..4 {
        let (mut cpu, mut bus) = fixture(&[0xe590_1000, 0xe580_2000], false);
        cpu.registers[0] = DATA + offset;
        cpu.registers[2] = 0xaabb_ccdd;
        write(&mut bus, DATA, 0x1234_5678);
        run(&mut cpu, &mut bus, 2);
        assert_eq!(cpu.registers[1], 0x1234_5678u32.rotate_right(offset * 8));
        assert_eq!(read(&mut bus, DATA), 0xaabb_ccdd);
    }
}

#[test]
fn arm_byte_load_store_and_postindex() {
    let (mut cpu, mut bus) = fixture(&[0xe4d0_1001, 0xe440_1001], false);
    cpu.registers[0] = DATA;
    write(&mut bus, DATA, 0x1234_56ab);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[0], DATA + 1);
    assert_eq!(cpu.registers[1], 0xab);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[0], DATA);
    assert_eq!(read(&mut bus, DATA), 0x1234_abab);
}

#[test]
fn arm_load_base_destination_overlap_keeps_loaded_value() {
    let (mut cpu, mut bus) = fixture(&[0xe490_0004], false);
    cpu.registers[0] = DATA;
    write(&mut bus, DATA, 0x1234);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[0], 0x1234);
}

#[test]
fn halfword_and_signed_load_alignment_quirks() {
    for (opcode, offset, expected) in [
        (0xe1d0_10b0, 0, 0x80fe),
        (0xe1d0_10b0, 1, 0xfe00_0080),
        (0xe1d0_10d0, 0, 0xffff_fffe),
        (0xe1d0_10f0, 0, 0xffff_80fe),
        (0xe1d0_10f0, 1, 0xffff_ff80),
    ] {
        let (mut cpu, mut bus) = fixture(&[opcode], false);
        cpu.registers[0] = DATA + offset;
        write(&mut bus, DATA, 0x1234_80fe);
        run(&mut cpu, &mut bus, 1);
        assert_eq!(
            cpu.registers[1], expected,
            "opcode {opcode:08x}, offset {offset}"
        );
    }
}

#[test]
fn halfword_store_immediate_preindex_and_register_offset() {
    // STRH r1, [r0, #2]!; LDRH r2, [r0, -r3].
    let (mut cpu, mut bus) = fixture(&[0xe1e0_10b2, 0xe110_20b3], false);
    cpu.registers[0] = DATA;
    cpu.registers[1] = 0x1234_abcd;
    cpu.registers[3] = 2;
    write(&mut bus, DATA, 0x5678);
    run(&mut cpu, &mut bus, 2);
    assert_eq!(cpu.registers[0], DATA + 2);
    assert_eq!(cpu.registers[2], 0x5678);
    assert_eq!(read(&mut bus, DATA), 0xabcd_5678);
}

#[test]
fn swap_reads_rotated_and_writes_unrotated() {
    let (mut cpu, mut bus) = fixture(&[0xe100_1092], false);
    cpu.registers[0] = DATA + 1;
    cpu.registers[2] = 0xaabb_ccdd;
    write(&mut bus, DATA, 0x1234_5678);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[1], 0x7812_3456);
    assert_eq!(read(&mut bus, DATA), 0xaabb_ccdd);
}

#[test]
fn multiply_and_accumulate_wrap_preserve_v_and_destroy_c() {
    // MULS r0, r1, r2; MLA r3, r1, r2, r4.
    let (mut cpu, mut bus) = fixture(&[0xe010_0291, 0xe023_4291], false);
    cpu.registers[1] = u32::MAX;
    cpu.registers[2] = 2;
    cpu.registers[4] = 10;
    cpu.status.set_carry(true);
    cpu.status.set_overflow(true);
    run(&mut cpu, &mut bus, 2);
    assert_eq!(cpu.registers[0], 0xffff_fffe);
    assert_eq!(cpu.registers[3], 8);
    assert!(
        !cpu.status.carry(),
        "reconstructed carry for this multiply is clear"
    );
    assert!(cpu.status.overflow());
}

#[test]
fn signed_unsigned_long_multiply_and_accumulation() {
    for (opcode, low, high) in [
        (0xe091_0392, 0xffff_fffe, 1),        // UMULLS r0,r1,r2,r3.
        (0xe0d1_0392, 0xffff_fffe, u32::MAX), // SMULLS.
        (0xe0b1_0392, 1, 2),                  // UMLALS, initial result=3.
        (0xe0f1_0392, 1, 0),                  // SMLALS, initial result=3.
    ] {
        let (mut cpu, mut bus) = fixture(&[opcode], false);
        cpu.registers[0] = 3;
        cpu.registers[2] = u32::MAX;
        cpu.registers[3] = 2;
        run(&mut cpu, &mut bus, 1);
        assert_eq!(
            (cpu.registers[0], cpu.registers[1]),
            (low, high),
            "{opcode:08x}"
        );
        assert_eq!(cpu.status.negative(), high >> 31 != 0);
        assert!(!cpu.status.zero());
    }
}

#[test]
fn long_multiply_zero_flag_uses_both_halves() {
    let (mut cpu, mut bus) = fixture(&[0xe091_0392], false);
    cpu.registers[2] = 0x10000;
    cpu.registers[3] = 0x10000;
    run(&mut cpu, &mut bus, 1);
    assert_eq!((cpu.registers[0], cpu.registers[1]), (0, 1));
    assert!(!cpu.status.zero());
}

#[test]
fn block_transfer_address_modes_and_register_order() {
    for (bits, start, final_base) in [
        (0x0080_0000, DATA, DATA + 12),      // IA.
        (0x0180_0000, DATA + 4, DATA + 12),  // IB.
        (0, DATA - 8, DATA - 12),            // DA.
        (0x0100_0000, DATA - 12, DATA - 12), // DB.
    ] {
        let (mut cpu, mut bus) = fixture(&[0xe820_000e | bits], false);
        cpu.registers[0] = DATA;
        cpu.registers[1..4].copy_from_slice(&[11, 22, 33]);
        run(&mut cpu, &mut bus, 1);
        assert_eq!(cpu.registers[0], final_base);
        for (index, value) in [11, 22, 33].into_iter().enumerate() {
            assert_eq!(read(&mut bus, start + index as u32 * 4), value);
        }
    }
}

#[test]
fn stm_base_in_list_stores_old_when_first_new_otherwise() {
    for (opcode, base_index, stored_at, expected) in [
        (0xe8a0_0003, 0, DATA, DATA),
        (0xe8a1_0003, 1, DATA + 4, DATA + 8),
    ] {
        let (mut cpu, mut bus) = fixture(&[opcode], false);
        cpu.registers[base_index] = DATA;
        run(&mut cpu, &mut bus, 1);
        assert_eq!(read(&mut bus, stored_at), expected);
        assert_eq!(cpu.registers[base_index], DATA + 8);
    }
}

#[test]
fn ldm_base_in_list_suppresses_writeback() {
    let (mut cpu, mut bus) = fixture(&[0xe8b0_0003], false);
    cpu.registers[0] = DATA;
    write(&mut bus, DATA, 123);
    write(&mut bus, DATA + 4, 456);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(&cpu.registers[..2], &[123, 456]);
}

#[test]
fn empty_arm_stm_transfers_pc_and_updates_base_by_64() {
    let (mut cpu, mut bus) = fixture(&[0xe8a0_0000], false);
    cpu.registers[0] = DATA;
    run(&mut cpu, &mut bus, 1);
    assert_eq!(read(&mut bus, DATA), START + 12);
    assert_eq!(cpu.registers[0], DATA + 64);
}

#[test]
fn user_bank_block_transfer_does_not_touch_irq_sp() {
    let (mut cpu, mut bus) = fixture(&[0xe8c0_6000, 0xe8d0_6000], false);
    cpu.registers[0] = DATA;
    cpu.registers[13] = 0x1111;
    cpu.registers[14] = 0x2222;
    cpu.replace_status(ProcessorStatus::try_new(0x12).expect("IRQ mode"));
    cpu.registers[13] = 0x3333;
    run(&mut cpu, &mut bus, 1);
    assert_eq!(read(&mut bus, DATA), 0x1111);
    write(&mut bus, DATA, 0x4444);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[13], 0x3333);
    cpu.replace_status(ProcessorStatus::try_new(0x1f).expect("System mode"));
    assert_eq!(cpu.registers[13], 0x4444);
}

#[test]
fn all_modes_preserve_banked_sp_lr_and_fiq_high_registers() {
    let (mut cpu, _) = fixture(&[], false);
    cpu.registers[8] = 88;
    cpu.registers[13] = 13;
    for mode in [0x11, 0x12, 0x13, 0x17, 0x1b] {
        cpu.replace_status(ProcessorStatus::try_new(mode).expect("valid mode"));
        cpu.registers[13] = mode * 100;
        cpu.registers[14] = mode * 100 + 1;
        if mode == 0x11 {
            cpu.registers[8] = 99;
        } else {
            assert_eq!(cpu.registers[8], 88);
        }
    }
    for mode in [0x11, 0x12, 0x13, 0x17, 0x1b] {
        cpu.replace_status(ProcessorStatus::try_new(mode).expect("valid mode"));
        assert_eq!(cpu.registers[13], mode * 100);
        assert_eq!(cpu.registers[14], mode * 100 + 1);
    }
    cpu.replace_status(ProcessorStatus::try_new(0x10).expect("User mode"));
    assert_eq!(cpu.registers[13], 13);
    assert_eq!(cpu.registers[8], 88);
}

#[test]
fn mrs_msr_register_immediate_and_user_privileges() {
    let (mut cpu, mut bus) = fixture(&[0xe121_f000, 0xe10f_1000, 0xe328_f102], false);
    cpu.registers[0] = 0x10; // MSR CPSR_c, r0 enters User.
    run(&mut cpu, &mut bus, 2);
    assert_eq!(cpu.status.mode(), ProcessorMode::User);
    assert_eq!(cpu.registers[1], 0x10);
    run(&mut cpu, &mut bus, 1); // MSR CPSR_f, #0x80000000.
    assert!(cpu.status.negative());
    cpu.arm_status_transfer(0xe121_f002)
        .expect("MSR in User is masked");
    assert_eq!(
        cpu.status.mode(),
        ProcessorMode::User,
        "User cannot switch mode"
    );
}

#[test]
fn msr_cannot_change_thumb_state() {
    let (mut cpu, mut bus) = fixture(&[0xe121_f000], false);
    cpu.registers[0] = 0x3f;
    run(&mut cpu, &mut bus, 1);
    assert!(!cpu.status.thumb());
    assert_eq!(cpu.status.mode(), ProcessorMode::System);
}

#[test]
fn swi_enters_supervisor_and_movs_pc_returns_to_thumb() {
    let (mut cpu, mut bus) = fixture(&[0xdf00, 0x202a], true);
    // BIOS vector: MOVS pc, lr.
    let mut bios = [0; 0x4000];
    bios[8..12].copy_from_slice(&0xe1b0_f00eu32.to_le_bytes());
    let mut new_bus = Bus::try_new(&bios, &[0; 192]).expect("valid BIOS");
    for (index, opcode) in [0xdf00, 0x202a].into_iter().enumerate() {
        new_bus
            .write(
                START + index as u32 * 2,
                AccessWidth::Halfword,
                opcode,
                AccessKind::Nonsequential,
            )
            .expect("test code write");
    }
    std::mem::swap(&mut bus, &mut new_bus);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.status.mode(), ProcessorMode::Supervisor);
    assert!(!cpu.status.thumb());
    assert_eq!(cpu.registers[14], START + 2);
    assert_eq!(cpu.address, 8);
    run(&mut cpu, &mut bus, 1);
    assert!(cpu.status.thumb());
    assert_eq!(cpu.status.mode(), ProcessorMode::System);
    assert_eq!(cpu.address, START + 2);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[0], 42);
}

#[test]
fn thumb_immediate_and_three_operand_arithmetic() {
    // MOV r0,#5; ADD r1,r0,#3; SUB r2,r1,r0; CMP r2,#3; ADD r2,#4; SUB r2,#1.
    let (mut cpu, mut bus) = fixture(&[0x2005, 0x1cc1, 0x1a0a, 0x2a03, 0x3204, 0x3a01], true);
    run(&mut cpu, &mut bus, 4);
    assert_eq!(&cpu.registers[..3], &[5, 8, 3]);
    assert!(cpu.status.zero());
    run(&mut cpu, &mut bus, 2);
    assert_eq!(cpu.registers[2], 6);
}

#[test]
fn thumb_all_alu_operation_results() {
    for (operation, expected, writeback) in [
        (0, 0, true),
        (1, 6, true),
        (2, 32, true),
        (3, 0, true),
        (4, 0, true),
        (5, 6, true),
        (6, 0xffff_fffd, true),
        (7, 0x2000_0000, true),
        (8, 0, false),
        (9, 0xffff_fffc, true),
        (10, 0xffff_fffe, false),
        (11, 6, false),
        (12, 6, true),
        (13, 8, true),
        (14, 2, true),
        (15, 0xffff_fffb, true),
    ] {
        // r0=2, r1=4, C=0. ADC=6, SBC=-3, ROR=0x20000000.
        let (mut cpu, mut bus) = fixture(&[0x4008 | (operation << 6)], true);
        cpu.registers[0] = 2;
        cpu.registers[1] = 4;
        run(&mut cpu, &mut bus, 1);
        assert_eq!(
            cpu.registers[0],
            if writeback { expected } else { 2 },
            "ALU operation {operation}"
        );
        assert_eq!(
            cpu.status.zero(),
            expected == 0,
            "ALU operation {operation}"
        );
    }
}

#[test]
fn thumb_high_mov_add_cmp_and_bx() {
    // MOV r8,r0; ADD r8,r1; CMP r8,r2; BX r3.
    let (mut cpu, mut bus) = fixture(&[0x4680, 0x4488, 0x4590, 0x4718], true);
    cpu.registers[..4].copy_from_slice(&[5, 7, 12, START + 16]);
    run(&mut cpu, &mut bus, 3);
    assert_eq!(cpu.registers[8], 12);
    assert!(cpu.status.zero());
    run(&mut cpu, &mut bus, 1);
    assert!(!cpu.status.thumb());
    assert_eq!(cpu.address, START + 16);
}

#[test]
fn thumb_pc_literal_alignment_and_address_generation() {
    // At +2, PC=+6, aligned PC=+4; LDR r0,[pc,#4] reads +8.
    let (mut cpu, mut bus) = fixture(&[0x46c0, 0x4801, 0xa101, 0xaa01, 0x5678, 0x1234], true);
    cpu.registers[13] = DATA;
    run(&mut cpu, &mut bus, 4);
    assert_eq!(cpu.registers[0], 0x1234_5678);
    assert_eq!(cpu.registers[1], START + 12);
    assert_eq!(cpu.registers[2], DATA + 4);
}

#[test]
fn thumb_register_memory_transfer_formats() {
    for (operation, expected) in [
        (3, 0xffff_fffe),
        (4, 0x1234_80fe),
        (5, 0x80fe),
        (6, 0xfe),
        (7, 0xffff_80fe),
    ] {
        let (mut cpu, mut bus) = fixture(&[0x5088 | (operation << 9)], true);
        cpu.registers[1] = DATA;
        cpu.registers[2] = 0;
        write(&mut bus, DATA, 0x1234_80fe);
        run(&mut cpu, &mut bus, 1);
        assert_eq!(cpu.registers[0], expected, "load operation {operation}");
    }
    for (operation, expected) in [(0, 0xabcd_1234), (1, 0xffff_1234), (2, 0xffff_ff34)] {
        let (mut cpu, mut bus) = fixture(&[0x5088 | (operation << 9)], true);
        cpu.registers[0] = 0xabcd_1234;
        cpu.registers[1] = DATA;
        write(&mut bus, DATA, u32::MAX);
        run(&mut cpu, &mut bus, 1);
        assert_eq!(
            read(&mut bus, DATA),
            expected,
            "store operation {operation}"
        );
    }
}

#[test]
fn thumb_immediate_memory_widths() {
    for (store, load, offset, expected) in [
        (0x6048, 0x684a, 4, 0x1234_5678),
        (0x7048, 0x784a, 1, 0x78),
        (0x8048, 0x884a, 2, 0x5678),
    ] {
        let (mut cpu, mut bus) = fixture(&[store, load], true);
        cpu.registers[0] = 0x1234_5678;
        cpu.registers[1] = DATA;
        run(&mut cpu, &mut bus, 2);
        assert_eq!(cpu.registers[2], expected, "offset {offset}");
        assert_eq!(
            read(&mut bus, DATA + (offset & !3)),
            expected << ((offset & 3) * 8)
        );
    }
}

#[test]
fn thumb_sp_relative_and_adjustment() {
    let (mut cpu, mut bus) = fixture(&[0x9001, 0x9901, 0xb001, 0xb082], true);
    cpu.registers[0] = 0x1234;
    cpu.registers[13] = DATA;
    run(&mut cpu, &mut bus, 4);
    assert_eq!(cpu.registers[1], 0x1234);
    assert_eq!(cpu.registers[13], DATA - 4);
}

#[test]
fn thumb_push_pop_pc_remains_thumb() {
    let (mut cpu, mut bus) = fixture(&[0xb503, 0xbd03], true);
    cpu.registers[..2].copy_from_slice(&[11, 22]);
    cpu.registers[13] = DATA + 12;
    cpu.registers[14] = START + 16; // Bit zero clear still returns to Thumb.
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[13], DATA);
    assert_eq!(read(&mut bus, DATA + 8), START + 16);
    cpu.registers[..2].fill(0);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(&cpu.registers[..2], &[11, 22]);
    assert_eq!(cpu.registers[13], DATA + 12);
    assert!(cpu.status.thumb());
    assert_eq!(cpu.address, START + 16);
}

#[test]
fn thumb_ldm_stm_and_base_overlap() {
    let (mut cpu, mut bus) = fixture(&[0xc003, 0xc803], true);
    cpu.registers[0] = DATA;
    cpu.registers[1] = 22;
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[0], DATA + 8);
    assert_eq!(read(&mut bus, DATA), DATA);
    cpu.registers[0] = DATA;
    run(&mut cpu, &mut bus, 1);
    assert_eq!(
        cpu.registers[0], DATA,
        "LDM loaded base suppresses writeback"
    );
    assert_eq!(cpu.registers[1], 22);
}

#[test]
fn thumb_conditional_and_unconditional_branches() {
    let (mut cpu, mut bus) = fixture(&[0x2002, 0x3801, 0xd1fd, 0xe000, 0x21ff, 0x212a], true);
    run(&mut cpu, &mut bus, 7);
    assert_eq!(cpu.registers[0], 0);
    assert_eq!(cpu.registers[1], 42);
}

#[test]
fn thumb_long_branch_link_pair() {
    let (mut cpu, mut bus) = fixture(&[0xf000, 0xf802, 0x2001, 0x2002, 0x2003], true);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[14], START + 4);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[14], START + 5);
    assert_eq!(cpu.address, START + 8);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[0], 3);
}

#[test]
fn thumb_long_branch_negative_first_half_sign_extends() {
    let (mut cpu, mut bus) = fixture(&[0xf7ff], true);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[14], (START + 4).wrapping_sub(4096));
}

#[test]
fn pipeline_retains_already_prefetched_instructions() {
    let (mut cpu, mut bus) = fixture(&[0xe1a0_0000, 0xe3a0_1001, 0xe3a0_2002], false);
    run(&mut cpu, &mut bus, 1);
    write(&mut bus, START + 4, 0xe3a0_1063);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(
        cpu.registers[1], 1,
        "instruction in decode stage must not be reread"
    );
    cpu.branch(START + 4);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.registers[1], 99, "branch refill observes modified code");
}

#[test]
fn baseline_cycles_include_pipeline_fill_and_internal_shift() {
    let (mut cpu, mut bus) = fixture(&[0xe1a0_0000, 0xe1a0_0000, 0xe1a0_0110, 0xeaff_fffe], false);
    let first = cpu.step(&mut bus).expect("NOP");
    assert_eq!(
        first.clock_after - first.clock_before,
        3,
        "two startup fetches plus one normal fetch"
    );
    let second = cpu.step(&mut bus).expect("NOP");
    assert_eq!(second.clock_after - second.clock_before, 1);
    let shift = cpu.step(&mut bus).expect("register shift");
    assert_eq!(shift.clock_after - shift.clock_before, 2);
    let branch = cpu.step(&mut bus).expect("branch");
    assert_eq!(branch.clock_after - branch.clock_before, 3);
}

#[test]
fn undefined_or_later_architecture_encodings_are_not_nops() {
    for opcode in [0xe12f_ff30, 0xe16f_0f10, 0xee10_0f10, 0xe600_0010] {
        let (mut cpu, mut bus) = fixture(&[opcode], false);
        run(&mut cpu, &mut bus, 1);
        assert_eq!(
            cpu.status.mode(),
            ProcessorMode::Undefined,
            "encoding {opcode:08x}"
        );
        assert_eq!(cpu.address, 4);
        assert_eq!(cpu.registers[14], START + 4);
    }
    let (mut cpu, mut bus) = fixture(&[0x4780], true);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.status.mode(), ProcessorMode::Undefined);
    assert_eq!(cpu.registers[14], START + 2);
}

#[test]
fn nv_condition_does_not_execute_armv5_unconditional_encodings() {
    let (mut cpu, mut bus) = fixture(&[0xf3a0_0001], false);
    let trace = cpu.step(&mut bus).expect("NV condition");
    assert!(!trace.condition_passed);
    assert_eq!(cpu.registers[0], 0);
}
