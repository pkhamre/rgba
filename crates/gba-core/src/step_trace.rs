use std::fmt;

/// One completed instruction, with post-execution state and elapsed clocks.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct StepTrace {
    /// Address of the executed instruction, not the pipelined R15 value.
    pub address: u32,
    /// Raw ARM opcode, or zero-extended Thumb opcode.
    pub opcode: u32,
    /// Instruction set used to decode this instruction.
    pub thumb: bool,
    /// Whether the ARM condition passed (always true for Thumb decoding).
    pub condition_passed: bool,
    /// Clock before this step, including any initial pipeline refill.
    pub clock_before: u64,
    /// Clock after this step.
    pub clock_after: u64,
    /// Post-execution R0–R14 and architectural pipelined R15.
    pub registers: [u32; 16],
    /// Post-execution CPSR.
    pub status: u32,
}

impl fmt::Display for StepTrace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "v1 {:012}..{:012} {:08x} {} {:08x} {} cpsr={:08x}",
            self.clock_before,
            self.clock_after,
            self.address,
            if self.thumb { 'T' } else { 'A' },
            self.opcode,
            if self.condition_passed {
                "exec"
            } else {
                "skip"
            },
            self.status
        )?;
        for (index, value) in self.registers.iter().enumerate() {
            write!(formatter, " r{index}={value:08x}")?;
        }
        Ok(())
    }
}
