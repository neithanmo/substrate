// Copyright 2017-2019 Parity Technologies (UK) Ltd.
// This file is part of Substrate.

// Substrate is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Substrate is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Substrate.  If not, see <http://www.gnu.org/licenses/>.

//! Different types of changes trie input pairs.

use codec::{Decode, Encode, Input, Output, Error};
use crate::changes_trie::BlockNumber;

/// Key of { changed key => set of extrinsic indices } mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtrinsicIndex<Number: BlockNumber> {
	/// Block at which this key has been inserted in the trie.
	pub block: Number,
	/// Storage key this node is responsible for.
	pub key: Vec<u8>,
}

/// Value of { changed key => set of extrinsic indices } mapping.
pub type ExtrinsicIndexValue = Vec<u32>;

/// Key of { changed key => block/digest block numbers } mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DigestIndex<Number: BlockNumber> {
	/// Block at which this key has been inserted in the trie.
	pub block: Number,
	/// Storage key this node is responsible for.
	pub key: Vec<u8>,
}

/// Key of { childtrie key => Childchange trie } mapping.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChildIndex<Number: BlockNumber> {
	/// Block at which this key has been inserted in the trie.
	pub block: Number,
	/// Storage key this node is responsible for.
	pub storage_key: Vec<u8>,
}

/// Value of { changed key => block/digest block numbers } mapping.
pub type DigestIndexValue<Number> = Vec<Number>;

/// Value of { changed key => block/digest block numbers } mapping.
/// That is the root of the child change trie.
pub type ChildIndexValue = Vec<u8>;

/// Single input pair of changes trie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputPair<Number: BlockNumber> {
	/// Element of { key => set of extrinsics where key has been changed } element mapping.
	ExtrinsicIndex(ExtrinsicIndex<Number>, ExtrinsicIndexValue),
	/// Element of { key => set of blocks/digest blocks where key has been changed } element mapping.
	DigestIndex(DigestIndex<Number>, DigestIndexValue<Number>),
	/// Element of { childtrie key => Childchange trie } where key has been changed } element mapping.
	ChildIndex(ChildIndex<Number>, ChildIndexValue),
}

/// Single input key of changes trie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputKey<Number: BlockNumber> {
	/// Key of { key => set of extrinsics where key has been changed } element mapping.
	ExtrinsicIndex(ExtrinsicIndex<Number>),
	/// Key of { key => set of blocks/digest blocks where key has been changed } element mapping.
	DigestIndex(DigestIndex<Number>),
	/// Key of { childtrie key => Childchange trie } where key has been changed } element mapping.
	ChildIndex(ChildIndex<Number>),
}

impl<Number: BlockNumber> Into<(Vec<u8>, Vec<u8>)> for InputPair<Number> {
	fn into(self) -> (Vec<u8>, Vec<u8>) {
		match self {
			InputPair::ExtrinsicIndex(key, value) => (key.encode(), value.encode()),
			InputPair::DigestIndex(key, value) => (key.encode(), value.encode()),
			InputPair::ChildIndex(key, value) => (key.encode(), value.encode()),
		}
	}
}

impl<Number: BlockNumber> Into<InputKey<Number>> for InputPair<Number> {
	fn into(self) -> InputKey<Number> {
		match self {
			InputPair::ExtrinsicIndex(key, _) => InputKey::ExtrinsicIndex(key),
			InputPair::DigestIndex(key, _) => InputKey::DigestIndex(key),
			InputPair::ChildIndex(key, _) => InputKey::ChildIndex(key),
		}
	}
}

impl<Number: BlockNumber> ExtrinsicIndex<Number> {
	pub fn key_neutral_prefix(block: Number) -> Vec<u8> {
		let mut prefix = vec![1];
		prefix.extend(block.encode());
		prefix
	}
}

impl<Number: BlockNumber> Encode for ExtrinsicIndex<Number> {
	fn encode_to<W: Output>(&self, dest: &mut W) {
		dest.push_byte(1);
		self.block.encode_to(dest);
		self.key.encode_to(dest);
	}
}

impl<Number: BlockNumber> codec::EncodeLike for ExtrinsicIndex<Number> {}

impl<Number: BlockNumber> DigestIndex<Number> {
	pub fn key_neutral_prefix(block: Number) -> Vec<u8> {
		let mut prefix = vec![2];
		prefix.extend(block.encode());
		prefix
	}
}


impl<Number: BlockNumber> Encode for DigestIndex<Number> {
	fn encode_to<W: Output>(&self, dest: &mut W) {
		dest.push_byte(2);
		self.block.encode_to(dest);
		self.key.encode_to(dest);
	}
}

impl<Number: BlockNumber> ChildIndex<Number> {
	pub fn key_neutral_prefix(block: Number) -> Vec<u8> {
		let mut prefix = vec![3];
		prefix.extend(block.encode());
		prefix
	}
}

impl<Number: BlockNumber> Encode for ChildIndex<Number> {
	fn encode_to<W: Output>(&self, dest: &mut W) {
		dest.push_byte(3);
		self.block.encode_to(dest);
		self.storage_key.encode_to(dest);
	}
}

impl<Number: BlockNumber> codec::EncodeLike for DigestIndex<Number> {}

impl<Number: BlockNumber> Decode for InputKey<Number> {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		match input.read_byte()? {
			1 => Ok(InputKey::ExtrinsicIndex(ExtrinsicIndex {
				block: Decode::decode(input)?,
				key: Decode::decode(input)?,
			})),
			2 => Ok(InputKey::DigestIndex(DigestIndex {
				block: Decode::decode(input)?,
				key: Decode::decode(input)?,
			})),
			3 => Ok(InputKey::ChildIndex(ChildIndex {
				block: Decode::decode(input)?,
				storage_key: Decode::decode(input)?,
			})),
			_ => Err("Invalid input key variant".into()),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn extrinsic_index_serialized_and_deserialized() {
		let original = ExtrinsicIndex { block: 777u64, key: vec![42] };
		let serialized = original.encode();
		let deserialized: InputKey<u64> = Decode::decode(&mut &serialized[..]).unwrap();
		assert_eq!(InputKey::ExtrinsicIndex(original), deserialized);
	}

	#[test]
	fn digest_index_serialized_and_deserialized() {
		let original = DigestIndex { block: 777u64, key: vec![42] };
		let serialized = original.encode();
		let deserialized: InputKey<u64> = Decode::decode(&mut &serialized[..]).unwrap();
		assert_eq!(InputKey::DigestIndex(original), deserialized);
	}
}
