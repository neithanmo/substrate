// Copyright 2019 Parity Technologies (UK) Ltd.
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

//! # Scored Pool Module
//!
//! The module maintains a scored membership pool. Each entity in the
//! pool can be attributed a `Score`. From this pool a set `Members`
//! is constructed. This set contains the `MemberCount` highest
//! scoring entities. Unscored entities are never part of `Members`.
//!
//! If an entity wants to be part of the pool a deposit is required.
//! The deposit is returned when the entity withdraws or when it
//! is removed by an entity with the appropriate authority.
//!
//! Every `Period` blocks the set of `Members` is refreshed from the
//! highest scoring members in the pool and, no matter if changes
//! occurred, `T::MembershipChanged::set_members_sorted` is invoked.
//! On first load `T::MembershipInitialized::initialize_members` is
//! invoked with the initial `Members` set.
//!
//! It is possible to withdraw candidacy/resign your membership at any
//! time. If an entity is currently a member, this results in removal
//! from the `Pool` and `Members`; the entity is immediately replaced
//! by the next highest scoring candidate in the pool, if available.
//!
//! - [`scored_pool::Trait`](./trait.Trait.html)
//! - [`Call`](./enum.Call.html)
//! - [`Module`](./struct.Module.html)
//!
//! ## Interface
//!
//! ### Public Functions
//!
//! - `submit_candidacy` - Submit candidacy to become a member. Requires a deposit.
//! - `withdraw_candidacy` - Withdraw candidacy. Deposit is returned.
//! - `score` - Attribute a quantitative score to an entity.
//! - `kick` - Remove an entity from the pool and members. Deposit is returned.
//! - `change_member_count` - Changes the amount of candidates taken into `Members`.
//!
//! ## Usage
//!
//! ```
//! use srml_support::{decl_module, dispatch::Result};
//! use system::ensure_signed;
//! use srml_scored_pool::{self as scored_pool};
//!
//! pub trait Trait: scored_pool::Trait {}
//!
//! decl_module! {
//! 	pub struct Module<T: Trait> for enum Call where origin: T::Origin {
//! 		pub fn candidate(origin) -> Result {
//! 			let who = ensure_signed(origin)?;
//!
//! 			let _ = <scored_pool::Module<T>>::submit_candidacy(
//! 				T::Origin::from(Some(who.clone()).into())
//! 			);
//! 			Ok(())
//! 		}
//! 	}
//! }
//!
//! # fn main() { }
//! ```
//!
//! ## Dependencies
//!
//! This module depends on the [System module](../srml_system/index.html).

// Ensure we're `no_std` when compiling for Wasm.
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

use codec::{Encode, Decode};
use rstd::prelude::*;
use srml_support::{
	StorageValue, StorageMap, decl_module, decl_storage, decl_event, ensure,
	traits::{ChangeMembers, InitializeMembers, Currency, Get, ReservableCurrency},
};
use system::{self, ensure_root, ensure_signed};
use sr_primitives::{
	traits::{EnsureOrigin, SimpleArithmetic, MaybeSerializeDebug, Zero, StaticLookup},
};

type BalanceOf<T, I> = <<T as Trait<I>>::Currency as Currency<<T as system::Trait>::AccountId>>::Balance;
type PoolT<T, I> = Vec<(<T as system::Trait>::AccountId, Option<<T as Trait<I>>::Score>)>;

/// The enum is supplied when refreshing the members set.
/// Depending on the enum variant the corresponding associated
/// type function will be invoked.
enum ChangeReceiver {
	/// Should call `T::MembershipInitialized`.
	MembershipInitialized,
	/// Should call `T::MembershipChanged`.
	MembershipChanged,
}

pub trait Trait<I=DefaultInstance>: system::Trait {
	/// The currency used for deposits.
	type Currency: Currency<Self::AccountId> + ReservableCurrency<Self::AccountId>;

	/// The score attributed to a member or candidate.
	type Score: SimpleArithmetic + Clone + Copy + Default + Encode + Decode + MaybeSerializeDebug;

	/// The overarching event type.
	type Event: From<Event<Self, I>> + Into<<Self as system::Trait>::Event>;

	// The deposit which is reserved from candidates if they want to
	// start a candidacy. The deposit gets returned when the candidacy is
	// withdrawn or when the candidate is kicked.
	type CandidateDeposit: Get<BalanceOf<Self, I>>;

	/// Every `Period` blocks the `Members` are filled with the highest scoring
	/// members in the `Pool`.
	type Period: Get<Self::BlockNumber>;

	/// The receiver of the signal for when the membership has been initialized.
	/// This happens pre-genesis and will usually be the same as `MembershipChanged`.
	/// If you need to do something different on initialization, then you can change
	/// this accordingly.
	type MembershipInitialized: InitializeMembers<Self::AccountId>;

	/// The receiver of the signal for when the members have changed.
	type MembershipChanged: ChangeMembers<Self::AccountId>;

	/// Allows a configurable origin type to set a score to a candidate in the pool.
	type ScoreOrigin: EnsureOrigin<Self::Origin>;

	/// Required origin for removing a member (though can always be Root).
	/// Configurable origin which enables removing an entity. If the entity
	/// is part of the `Members` it is immediately replaced by the next
	/// highest scoring candidate, if available.
	type KickOrigin: EnsureOrigin<Self::Origin>;
}

decl_storage! {
	trait Store for Module<T: Trait<I>, I: Instance=DefaultInstance> as ScoredPool {
		/// The current pool of candidates, stored as an ordered Vec
		/// (ordered descending by score, `None` last, highest first).
		Pool get(pool) config(): PoolT<T, I>;

		/// A Map of the candidates. The information in this Map is redundant
		/// to the information in the `Pool`. But the Map enables us to easily
		/// check if a candidate is already in the pool, without having to
		/// iterate over the entire pool (the `Pool` is not sorted by
		/// `T::AccountId`, but by `T::Score` instead).
		CandidateExists get(candidate_exists): map T::AccountId => bool;

		/// The current membership, stored as an ordered Vec.
		Members get(members): Vec<T::AccountId>;

		/// Size of the `Members` set.
		MemberCount get(member_count) config(): u32;
	}
	add_extra_genesis {
		config(members): Vec<T::AccountId>;
		config(phantom): rstd::marker::PhantomData<I>;
		build(|config| {
			let mut pool = config.pool.clone();

			// reserve balance for each candidate in the pool.
			// panicking here is ok, since this just happens one time, pre-genesis.
			pool
				.iter()
				.for_each(|(who, _)| {
					T::Currency::reserve(&who, T::CandidateDeposit::get())
						.expect("balance too low to create candidacy");
					<CandidateExists<T, I>>::insert(who, true);
				});

			/// Sorts the `Pool` by score in a descending order. Entities which
			/// have a score of `None` are sorted to the beginning of the vec.
			pool.sort_by_key(|(_, maybe_score)|
				Reverse(maybe_score.unwrap_or_default())
			);

			<Pool<T, I>>::put(&pool);
			<Module<T, I>>::refresh_members(pool, ChangeReceiver::MembershipInitialized);
		})
	}
}

decl_event!(
	pub enum Event<T, I=DefaultInstance> where
		<T as system::Trait>::AccountId,
	{
		/// The given member was removed. See the transaction for who.
		MemberRemoved,
		/// An entity has issued a candidacy. See the transaction for who.
		CandidateAdded,
		/// An entity withdrew candidacy. See the transaction for who.
		CandidateWithdrew,
		/// The candidacy was forcefully removed for an entity.
		/// See the transaction for who.
		CandidateKicked,
		/// A score was attributed to the candidate.
		/// See the transaction for who.
		CandidateScored,
		/// Phantom member, never used.
		Dummy(rstd::marker::PhantomData<(AccountId, I)>),
	}
);

decl_module! {
	pub struct Module<T: Trait<I>, I: Instance=DefaultInstance>
		for enum Call
		where origin: T::Origin
	{
		fn deposit_event() = default;

		/// Every `Period` blocks the `Members` set is refreshed from the
		/// highest scoring members in the pool.
		fn on_initialize(n: T::BlockNumber) {
			if n % T::Period::get() == Zero::zero() {
				let pool = <Pool<T, I>>::get();
				<Module<T, I>>::refresh_members(pool, ChangeReceiver::MembershipChanged);
			}
		}

		/// Add `origin` to the pool of candidates.
		///
		/// This results in `CandidateDeposit` being reserved from
		/// the `origin` account. The deposit is returned once
		/// candidacy is withdrawn by the candidate or the entity
		/// is kicked by `KickOrigin`.
		///
		/// The dispatch origin of this function must be signed.
		///
		/// The `index` parameter of this function must be set to
		/// the index of the transactor in the `Pool`.
		pub fn submit_candidacy(origin) {
			let who = ensure_signed(origin)?;
			ensure!(!<CandidateExists<T, I>>::exists(&who), "already a member");

			let deposit = T::CandidateDeposit::get();
			T::Currency::reserve(&who, deposit)
				.map_err(|_| "balance too low to submit candidacy")?;

			// can be inserted as last element in pool, since entities with
			// `None` are always sorted to the end.
			if let Err(e) = <Pool<T, I>>::append(&[(who.clone(), None)]) {
				T::Currency::unreserve(&who, deposit);
				return Err(e);
			}

			<CandidateExists<T, I>>::insert(&who, true);

			Self::deposit_event(RawEvent::CandidateAdded);
		}

		/// An entity withdraws candidacy and gets its deposit back.
		///
		/// If the entity is part of the `Members`, then the highest member
		/// of the `Pool` that is not currently in `Members` is immediately
		/// placed in the set instead.
		///
		/// The dispatch origin of this function must be signed.
		///
		/// The `index` parameter of this function must be set to
		/// the index of the transactor in the `Pool`.
		pub fn withdraw_candidacy(
			origin,
			index: u32
		) {
			let who = ensure_signed(origin)?;

			let pool = <Pool<T, I>>::get();
			Self::ensure_index(&pool, &who, index)?;

			Self::remove_member(pool, who, index)?;
			Self::deposit_event(RawEvent::CandidateWithdrew);
		}

		/// Kick a member `who` from the set.
		///
		/// May only be called from `KickOrigin` or root.
		///
		/// The `index` parameter of this function must be set to
		/// the index of `dest` in the `Pool`.
		pub fn kick(
			origin,
			dest: <T::Lookup as StaticLookup>::Source,
			index: u32
		) {
			T::KickOrigin::try_origin(origin)
				.map(|_| ())
				.or_else(ensure_root)
				.map_err(|_| "bad origin")?;

			let who = T::Lookup::lookup(dest)?;

			let pool = <Pool<T, I>>::get();
			Self::ensure_index(&pool, &who, index)?;

			Self::remove_member(pool, who, index)?;
			Self::deposit_event(RawEvent::CandidateKicked);
		}

		/// Score a member `who` with `score`.
		///
		/// May only be called from `ScoreOrigin` or root.
		///
		/// The `index` parameter of this function must be set to
		/// the index of the `dest` in the `Pool`.
		pub fn score(
			origin,
			dest: <T::Lookup as StaticLookup>::Source,
			index: u32,
			score: T::Score
		) {
			T::ScoreOrigin::try_origin(origin)
				.map(|_| ())
				.or_else(ensure_root)
				.map_err(|_| "bad origin")?;

			let who = T::Lookup::lookup(dest)?;

			let mut pool = <Pool<T, I>>::get();
			Self::ensure_index(&pool, &who, index)?;

			pool.remove(index as usize);

			// we binary search the pool (which is sorted descending by score).
			// if there is already an element with `score`, we insert
			// right before that. if not, the search returns a location
			// where we can insert while maintaining order.
			let item = (who.clone(), Some(score.clone()));
			let location = pool
				.binary_search_by_key(
					&Reverse(score),
					|(_, maybe_score)| Reverse(maybe_score.unwrap_or_default())
				)
				.unwrap_or_else(|l| l);
			pool.insert(location, item);

			<Pool<T, I>>::put(&pool);
			Self::deposit_event(RawEvent::CandidateScored);
		}

		/// Dispatchable call to change `MemberCount`.
		///
		/// This will only have an effect the next time a refresh happens
		/// (this happens each `Period`).
		///
		/// May only be called from root.
		pub fn change_member_count(origin, count: u32) {
			ensure_root(origin)?;
			<MemberCount<I>>::put(&count);
		}
	}
}

impl<T: Trait<I>, I: Instance> Module<T, I> {

	/// Fetches the `MemberCount` highest scoring members from
	/// `Pool` and puts them into `Members`.
	///
	/// The `notify` parameter is used to deduct which associated
	/// type function to invoke at the end of the method.
	fn refresh_members(
		pool: PoolT<T, I>,
		notify: ChangeReceiver
	) {
		let count = <MemberCount<I>>::get();

		let mut new_members: Vec<T::AccountId> = pool
			.into_iter()
			.filter(|(_, score)| score.is_some())
			.take(count as usize)
			.map(|(account_id, _)| account_id)
			.collect();
		new_members.sort();

		let old_members = <Members<T, I>>::get();
		<Members<T, I>>::put(&new_members);

		match notify {
			ChangeReceiver::MembershipInitialized =>
				T::MembershipInitialized::initialize_members(&new_members),
			ChangeReceiver::MembershipChanged =>
				T::MembershipChanged::set_members_sorted(
					&new_members[..],
					&old_members[..],
				),
		}
	}

	/// Removes an entity `remove` at `index` from the `Pool`.
	///
	/// If the entity is a member it is also removed from `Members` and
	/// the deposit is returned.
	fn remove_member(
		mut pool: PoolT<T, I>,
		remove: T::AccountId,
		index: u32
	) -> Result<(), &'static str> {
		// all callers of this function in this module also check
		// the index for validity before calling this function.
		// nevertheless we check again here, to assert that there was
		// no mistake when invoking this sensible function.
		Self::ensure_index(&pool, &remove, index)?;

		pool.remove(index as usize);
		<Pool<T, I>>::put(&pool);

		// remove from set, if it was in there
		let members = <Members<T, I>>::get();
		if members.binary_search(&remove).is_ok() {
			Self::refresh_members(pool, ChangeReceiver::MembershipChanged);
		}

		<CandidateExists<T, I>>::remove(&remove);

		T::Currency::unreserve(&remove, T::CandidateDeposit::get());

		Self::deposit_event(RawEvent::MemberRemoved);
		Ok(())
	}

	/// Checks if `index` is a valid number and if the element found
	/// at `index` in `Pool` is equal to `who`.
	fn ensure_index(
		pool: &PoolT<T, I>,
		who: &T::AccountId,
		index: u32
	) -> Result<(), &'static str> {
		ensure!(index < pool.len() as u32, "index out of bounds");

		let (index_who, _index_score) = &pool[index as usize];
		ensure!(index_who == who, "index does not match requested account");

		Ok(())
	}
}

