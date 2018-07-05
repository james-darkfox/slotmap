use std;
use std::fmt;
use std::mem::ManuallyDrop;

#[cfg(feature = "serde")]
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

// Little helper function to turn (bool, T) into Option<T>.
fn to_option<T>(b: bool, some: T) -> Option<T> {
    match b {
        true => Some(some),
        false => None,
    }
}


// A slot, which represents storage for a value and a current version.
// Can be occupied or vacant
pub struct Slot<T> {
    // A value when occupied, uninitialized memory otherwise.
    value: ManuallyDrop<T>,

    // Even = vacant, odd = occupied.
    version: u32,

    // This could be in an union with value, but that requires unions for types
    // without copy. This isn't available in stable Rust yet.
    pub next_free: u32,
}

impl<T> Slot<T> {
    pub fn new() -> Self {
        Self {
            value: unsafe { std::mem::uninitialized() },
            version: 0,
            next_free: 0,
        }
    }

    // Is this slot occupied?
    pub fn occupied(&self) -> bool {
        self.version % 2 > 0
    }

    // Get an OccupiedVersion for this slot. If the slot is currently unoccupied
    // it will return the version it would have when it gets occupied.
    pub fn occupied_version(&self) -> u32 {
        self.version | 1
    }

    // Checks the slot's version for equality. If this returns true you also
    // know the slot is occupied.
    pub fn has_version(&self, version: u32) -> bool {
        self.version == version
    }

    // Get the slot's value, if occupied.
    pub fn value(&self) -> Option<&T> {
        to_option(self.occupied(), &self.value)
    }

    pub fn value_mut(&mut self) -> Option<&mut T> {
        let occupied = self.occupied();
        to_option(occupied, &mut self.value)
    }

    // Get the slot's value, if occupied and the correct version is given.
    pub fn get_versioned(&self, version: u32) -> Option<&T> {
        let correct_version = self.has_version(version);
        to_option(correct_version, &self.value)
    }

    pub fn get_versioned_mut(&mut self, version: u32) -> Option<&mut T> {
        let correct_version = self.has_version(version);
        to_option(correct_version, &mut self.value)
    }

    // Get the slot's value without any safety checks.
    pub unsafe fn get_unchecked(&self) -> &T {
        &self.value
    }
    pub unsafe fn get_unchecked_mut(&mut self) -> &mut T {
        &mut self.value
    }

    // Store a new value. Must be unoccupied before storing.
    pub unsafe fn store_value(&mut self, value: T) {
        self.version |= 1;
        self.value = ManuallyDrop::new(value);
    }

    // Remove a stored value. Must be occupied before removing.
    pub unsafe fn remove_value(&mut self) -> T {
        self.version = self.version.wrapping_add(1);
        std::mem::replace(&mut *self.value, std::mem::uninitialized())
    }
}

impl<T> Drop for Slot<T> {
    fn drop(&mut self) {
        if self.occupied() {
            unsafe {
                ManuallyDrop::drop(&mut self.value);
            }
        }
    }
}

impl<T> Clone for Slot<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Slot::<T> {
            value: if self.occupied() {
                self.value.clone()
            } else {
                unsafe { std::mem::uninitialized() }
            },
            version: self.version,
            next_free: self.next_free,
        }
    }
}

impl<T> fmt::Debug for Slot<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let mut builder = fmt.debug_struct("Slot");
        builder.field("version", &self.version);
        if self.occupied() {
            builder.field("value", &self.value).finish()
        } else {
            builder.field("next_free", &self.next_free).finish()
        }
    }
}

// Serialization.
#[cfg(feature = "serde")]
#[derive(Serialize, Deserialize)]
struct SafeSlot<T> {
    value: Option<T>,
    version: u32,
}

#[cfg(feature = "serde")]
impl<'a, T> From<SafeSlot<T>> for Slot<T> {
    fn from(safe_slot: SafeSlot<T>) -> Self {
        Slot {
            value: match safe_slot.value {
                Some(value) => ManuallyDrop::new(value),
                None => unsafe { std::mem::uninitialized() },
            },
            version: safe_slot.version,
            next_free: 0,
        }
    }
}

#[cfg(feature = "serde")]
impl<'a, T> From<&'a Slot<T>> for SafeSlot<&'a T> {
    fn from(slot: &'a Slot<T>) -> Self {
        SafeSlot {
            value: slot.value(),
            version: slot.version,
        }
    }
}

#[cfg(feature = "serde")]
impl<T> Serialize for Slot<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        SafeSlot::from(self).serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, T> Deserialize<'de> for Slot<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let safe_slot: SafeSlot<T> = Deserialize::deserialize(deserializer)?;
        let occupied = safe_slot.version % 2 > 0;
        if occupied ^ safe_slot.value.is_some() {
            return Err(de::Error::custom(&"inconsistent occupation in Slot"));
        }

        Ok(Slot::from(safe_slot))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "serde")]
    use serde_json;

    #[cfg(feature = "serde")]
    #[test]
    fn slot_serde() {
        let slot = Slot {
            value: ManuallyDrop::new("test"),
            version: 1,
            next_free: 42,
        };

        let ser = serde_json::to_string(&slot).unwrap();
        let de: Slot<&str> = serde_json::from_str(&ser).unwrap();
        assert_eq!(de.value, slot.value);
        assert_eq!(de.version, slot.version);
        assert_eq!(de.next_free, 0); // next_free should not survive serialization.
    }
}
