use crate::utils::types::*;
use anyhow::Error;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Determines the present slot based upon a manually-incremented UNIX timestamp.
/// based on: https://github.com/sigp/lighthouse/blob/stable/common/slot_clock/src/manual_slot_clock.rs
pub struct SlotClock {
    genesis_slot: Slot,
    /// Duration from UNIX epoch to genesis.
    genesis_duration: Duration,
    /// The length of each slot.
    slot_duration: Duration,
    slots_per_epoch: u64,
    l2_slot_duration_sec: u64,
}

impl SlotClock {
    pub fn new(
        genesis_slot: Slot,
        genesis_timestamp_sec: u64,
        slot_duration_sec: u64,
        slots_per_epoch: u64,
        l2_slot_duration_sec: u64,
    ) -> Self {
        tracing::info!(
            "SlotClock: genesis_timestamp_sec: {}, genesis_slot: {}",
            genesis_timestamp_sec,
            genesis_slot
        );

        let slot_duration = Duration::from_secs(slot_duration_sec);
        Self {
            genesis_slot,
            genesis_duration: Duration::from_secs(genesis_timestamp_sec) - slot_duration,
            slot_duration,
            slots_per_epoch,
            l2_slot_duration_sec,
        }
    }

    pub fn get_slots_per_epoch(&self) -> u64 {
        self.slots_per_epoch
    }

    pub fn duration_to_next_slot(&self) -> Result<Duration, Error> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
        self.duration_to_next_slot_from(now)
    }

    pub fn get_current_slot(&self) -> Result<Slot, Error> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
        self.slot_of(now)
    }

    /// Returns the duration between `now` and the start of the next slot.
    pub fn duration_to_next_slot_from(&self, now: Duration) -> Result<Duration, Error> {
        if now < self.genesis_duration {
            Ok(self
                .genesis_duration
                .checked_sub(now)
                .ok_or(anyhow::anyhow!(
                    "duration_to_next_slot_from: Subtraction overflow"
                ))?)
        } else {
            self.duration_to_slot(self.slot_of(now)? + 1, now)
        }
    }

    pub fn duration_to_slot_from_now(&self, slot: Slot) -> Result<Duration, Error> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
        self.duration_to_slot(slot, now)
    }

    pub fn slot_of(&self, now: Duration) -> Result<Slot, Error> {
        let genesis: Duration = self.genesis_duration;

        if now >= genesis {
            let since_genesis = now
                .checked_sub(genesis)
                .ok_or(anyhow::anyhow!("slot_of: Subtraction overflow"))?;
            let slot =
                Slot::from((since_genesis.as_millis() / self.slot_duration.as_millis()) as u64);
            Ok(slot + self.genesis_slot)
        } else {
            Err(anyhow::anyhow!("slot_of: now is less than genesis"))
        }
    }

    /// Returns the duration from `now` until the start of `slot`.
    ///
    /// Will return `None` if `now` is later than the start of `slot`.
    pub fn duration_to_slot(&self, slot: Slot, now: Duration) -> Result<Duration, Error> {
        self.start_of(slot)?
            .checked_sub(now)
            .ok_or(anyhow::anyhow!("duration_to_slot: Subtraction overflow"))
    }

    /// Returns the duration between UNIX epoch and the start of `slot`.
    pub fn start_of(&self, slot: Slot) -> Result<Duration, Error> {
        let slot = slot
            .checked_sub(self.genesis_slot)
            .ok_or(anyhow::anyhow!("start_of: Slot is less than genesis slot"))?
            .try_into()?;
        let unadjusted_slot_duration = self
            .slot_duration
            .checked_mul(slot)
            .ok_or(anyhow::anyhow!("start_of: Multiplication overflow"))?;

        self.genesis_duration
            .checked_add(unadjusted_slot_duration)
            .ok_or(anyhow::anyhow!("start_of: Addition overflow"))
    }

    /// Calculates the current epoch from the genesis time and current time.
    pub fn get_current_epoch(&self) -> Result<Epoch, Error> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
        let slot = self.slot_of(now)?;
        Ok(slot / self.slots_per_epoch)
    }

    // returns real timestamp, the shift is reduced
    pub fn get_real_epoch_begin_timestamp_for_contract(&self, epoch: Epoch) -> Result<u64, Error> {
        let slot = epoch * self.slots_per_epoch;
        let start_of_slot = self.start_of(slot)? + self.slot_duration;
        Ok(start_of_slot.as_secs())
    }

    // returns real timestamp, the shift is reduced
    pub fn get_real_slot_begin_timestamp_for_contract(&self, slot: Slot) -> Result<u64, Error> {
        let start_of_slot = self.start_of(slot)? + self.slot_duration;
        Ok(start_of_slot.as_secs())
    }

    pub fn get_epoch_for_timestamp(&self, timestamp: u64) -> Result<Epoch, Error> {
        let slot = self.slot_of(Duration::from_secs(timestamp))?;
        Ok(slot / self.slots_per_epoch)
    }

    pub fn get_current_slot_of_epoch(&self) -> Result<Slot, Error> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
        let cur_slot = self.slot_of(now)?;
        Ok(self.slot_of_epoch(cur_slot))
    }

    pub fn slot_of_epoch(&self, slot: Slot) -> Slot {
        slot % self.slots_per_epoch
    }

    // 0 based L2 slot number within the current L1 slot
    pub fn get_l2_slot_number_within_l1_slot(&self) -> Result<u64, Error> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
        let slot_begin = self.start_of(self.get_current_slot()?)?;
        Ok(self.which_l2_slot_is_it((now - slot_begin).as_secs()))
    }

    fn which_l2_slot_is_it(&self, secs_from_l1_slot_begin: u64) -> u64 {
        secs_from_l1_slot_begin / self.l2_slot_duration_sec
    }
}

#[cfg(test)]
mod tests {
    use ethereum_consensus::phase0::mainnet::SLOTS_PER_EPOCH;

    use super::*;

    const SLOT_DURATION: u64 = 12;
    const L2_SLOT_DURATION: u64 = 3;

    #[test]
    fn test_duration_to_next_slot() {
        let genesis_slot = Slot::from(0u64);
        let slot_clock = SlotClock::new(
            genesis_slot,
            SLOT_DURATION,
            SLOT_DURATION,
            32,
            L2_SLOT_DURATION,
        );

        let now = Duration::from_secs(10);
        let duration_to_next_slot = slot_clock.duration_to_next_slot_from(now).unwrap();
        assert_eq!(duration_to_next_slot, Duration::from_secs(2));
    }

    #[test]
    fn test_slot_of() {
        let genesis_slot = Slot::from(0u64);
        let slot_clock = SlotClock::new(
            genesis_slot,
            SLOT_DURATION,
            SLOT_DURATION,
            32,
            L2_SLOT_DURATION,
        );

        let now = Duration::from_secs(25);
        let slot = slot_clock.slot_of(now).unwrap();
        assert_eq!(slot, Slot::from(2u64));
    }

    #[test]
    fn test_duration_to_slot() {
        let genesis_slot = Slot::from(0u64);
        let slot_clock = SlotClock::new(
            genesis_slot,
            SLOT_DURATION,
            SLOT_DURATION,
            32,
            L2_SLOT_DURATION,
        );

        let now = Duration::from_secs(10);
        let slot = Slot::from(2u64);
        let duration_to_slot = slot_clock.duration_to_slot(slot, now).unwrap();
        assert_eq!(duration_to_slot, Duration::from_secs(14));
    }

    #[test]
    fn test_start_of() {
        let genesis_slot = Slot::from(0u64);
        let slot_clock = SlotClock::new(
            genesis_slot,
            SLOT_DURATION,
            SLOT_DURATION,
            32,
            L2_SLOT_DURATION,
        );

        let start_of_slot = slot_clock.start_of(Slot::from(3u64)).unwrap();
        assert_eq!(start_of_slot, Duration::from_secs(36));
    }

    #[test]
    fn test_get_current_slot() {
        let genesis_slot = Slot::from(0u64);
        let slot_clock = SlotClock::new(
            genesis_slot,
            1721387493,
            SLOT_DURATION,
            32,
            L2_SLOT_DURATION,
        );

        let current_slot = slot_clock.get_current_slot().unwrap();
        println!("current_slot: {}", current_slot);
        assert!(current_slot > genesis_slot);
    }

    #[test]
    fn test_get_current_epoch() {
        let genesis_slot = Slot::from(0u64);
        let slot_clock = SlotClock::new(
            genesis_slot,
            1721387493,
            SLOT_DURATION,
            32,
            L2_SLOT_DURATION,
        );

        let current_epoch = slot_clock.get_current_epoch().unwrap();
        assert!(current_epoch > 0);
    }

    #[test]
    fn test_get_epoch_begin_timestamp() {
        let genesis_slot = Slot::from(0u64);
        let genesis_timestamp = 100;
        let slot_duration = SLOT_DURATION;
        let slot_per_epoch = SLOTS_PER_EPOCH;
        let slot_clock = SlotClock::new(
            genesis_slot,
            genesis_timestamp,
            slot_duration,
            slot_per_epoch,
            L2_SLOT_DURATION,
        );

        let epoch_begin_timestamp = slot_clock
            .get_real_epoch_begin_timestamp_for_contract(1)
            .unwrap();
        assert_eq!(
            epoch_begin_timestamp,
            genesis_timestamp + slot_per_epoch * slot_duration
        );
    }

    #[test]
    fn test_get_current_slot_of_epoch() {
        let genesis_slot = Slot::from(0u64);
        let slot_clock = SlotClock::new(genesis_slot, 100, SLOT_DURATION, 32, L2_SLOT_DURATION);

        assert_eq!(slot_clock.slot_of_epoch(1234), 18);
        assert_eq!(slot_clock.slot_of_epoch(293482), 10);
    }
}
