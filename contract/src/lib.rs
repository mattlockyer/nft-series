mod utils;
use crate::utils::*;

use std::collections::HashMap;
use near_contract_standards::non_fungible_token::metadata::{
    NFTContractMetadata, NonFungibleTokenMetadataProvider, TokenMetadata, NFT_METADATA_SPEC,
};
use near_contract_standards::non_fungible_token::{Token, TokenId};
use near_contract_standards::non_fungible_token::core::{
	NonFungibleTokenCore, NonFungibleTokenResolver
};
use near_contract_standards::non_fungible_token::NonFungibleToken;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, Vector, LookupMap, UnorderedMap, UnorderedSet};
use near_sdk::json_types::{U64, U128};
use near_sdk::{
    env, near_bindgen, serde_json::json, Balance, AccountId, BorshStorageKey, PanicOnDefault, Promise, PromiseOrValue,
};
use near_sdk::serde::{Deserialize, Serialize};

/// CUSTOM TYPES

/// log series const
pub const EVENT_JSON: &str = "EVENT_JSON:";
/// between token_series_id and edition number e.g. 42:2 where 42 is series and 2 is edition
pub const TOKEN_DELIMETER: char = ':';
/// TokenMetadata.title returned for individual token e.g. "Title — 2/10" where 10 is max copies
pub const TITLE_DELIMETER: &str = " — ";
/// e.g. "Title — 2/10" where 10 is max copies
pub const EDITION_DELIMETER: &str = "/";
pub type TokenSeriesId = u64;
pub type TokenSeriesTitle = String;
#[derive(BorshDeserialize, BorshSerialize)]
pub struct TokenSeries {
	metadata: TokenMetadata,
	owner_id: AccountId,
	royalty: HashMap<AccountId, u32>,
	tokens: UnorderedSet<TokenId>,
	approved_market_id: Option<AccountId>,
}
#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct TokenSeriesJson {
	metadata: TokenMetadata,
	owner_id: AccountId,
	royalty: HashMap<AccountId, u32>,
}
#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct SeriesMintArgs {
	token_series_title: TokenSeriesTitle,
	receiver_id: AccountId,
}

/// payout series for royalties to market
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Payout {
	payout: HashMap<AccountId, U128>
}

/// STANDARD
#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Contract {
    tokens: NonFungibleToken,
    metadata: LazyOption<NFTContractMetadata>,
	// CUSTOM
	token_series_by_title: LookupMap<TokenSeriesTitle, TokenSeriesId>,
	token_series_by_id: UnorderedMap<TokenSeriesId, TokenSeries>,
}
const DATA_IMAGE_SVG_NEAR_ICON: &str = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 288 288'%3E%3Cg id='l' data-name='l'%3E%3Cpath d='M187.58,79.81l-30.1,44.69a3.2,3.2,0,0,0,4.75,4.2L191.86,103a1.2,1.2,0,0,1,2,.91v80.46a1.2,1.2,0,0,1-2.12.77L102.18,77.93A15.35,15.35,0,0,0,90.47,72.5H87.34A15.34,15.34,0,0,0,72,87.84V201.16A15.34,15.34,0,0,0,87.34,216.5h0a15.35,15.35,0,0,0,13.08-7.31l30.1-44.69a3.2,3.2,0,0,0-4.75-4.2L96.14,186a1.2,1.2,0,0,1-2-.91V104.61a1.2,1.2,0,0,1,2.12-.77l89.55,107.23a15.35,15.35,0,0,0,11.71,5.43h3.13A15.34,15.34,0,0,0,216,201.16V87.84A15.34,15.34,0,0,0,200.66,72.5h0A15.35,15.35,0,0,0,187.58,79.81Z'/%3E%3C/g%3E%3C/svg%3E";
#[derive(BorshSerialize, BorshStorageKey)]
enum StorageKey {
	// STANDARD
    NonFungibleToken,
    Metadata,
    TokenMetadata,
    Enumeration,
    Approval,
	// CUSTOM
    TokenSeriesByTitle,
    TokenSeriesById,
    TokensBySeriesInner { token_series_id: u64 },
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new_default_meta(owner_id: AccountId) -> Self {
        Self::new(
            owner_id,
            NFTContractMetadata {
                spec: NFT_METADATA_SPEC.to_string(),
                name: "NFT Series".to_string(),
                symbol: "NFT".to_string(),
                icon: Some(DATA_IMAGE_SVG_NEAR_ICON.to_string()),
                base_uri: None,
                reference: None,
                reference_hash: None,
            },
        )
    }

    #[init]
    pub fn new(owner_id: AccountId, metadata: NFTContractMetadata) -> Self {
        assert!(!env::state_exists(), "Already initialized");
        metadata.assert_valid();
        Self {
            tokens: NonFungibleToken::new(
                StorageKey::NonFungibleToken,
                owner_id,
                Some(StorageKey::TokenMetadata),
                Some(StorageKey::Enumeration),
                Some(StorageKey::Approval),
            ),
			token_series_by_id: UnorderedMap::new(StorageKey::TokenSeriesById),
			token_series_by_title: LookupMap::new(StorageKey::TokenSeriesByTitle),
            metadata: LazyOption::new(StorageKey::Metadata, Some(&metadata)),
        }
    }

	// CUSTOM
    
    #[payable]
    pub fn nft_create_series(
        &mut self,
        metadata: TokenMetadata,
        royalty: HashMap<AccountId, u32>,
    ) {
		let initial_storage_usage = env::storage_usage();
        let owner_id = env::predecessor_account_id();
		let title = metadata.title.clone();
		assert!(title.is_some(), "token_metadata.title is required");
		let token_series_id = self.token_series_by_id.len() + 1;
        assert!(self.token_series_by_title.insert(&title.unwrap(), &token_series_id).is_none(), "token_metadata.title exists");
        self.token_series_by_id.insert(&token_series_id, &TokenSeries{
			metadata,
			owner_id,
			royalty,
			tokens: UnorderedSet::new(
				StorageKey::TokensBySeriesInner {
					token_series_id
				}
				.try_to_vec()
				.unwrap(),
			),
			approved_market_id: None,
		});

        refund_deposit(env::storage_usage() - initial_storage_usage);
    }

	pub fn cap_copies(
		&mut self,
		token_series_title: TokenSeriesTitle,
	) {
		assert_eq!(env::predecessor_account_id(), self.tokens.owner_id, "Unauthorized");
		let token_series_id = self.token_series_by_title.get(&token_series_title).expect("no series");
		let mut token_series = self.token_series_by_id.get(&token_series_id).expect("no token");
		token_series.metadata.copies = Some(token_series.tokens.len());
		self.token_series_by_id.insert(&token_series_id, &token_series);
	}

	#[payable]
	pub fn nft_mint_series(
		&mut self,
		token_series_title: TokenSeriesTitle,
		receiver_id: AccountId,
	) -> Token {
		let initial_storage_usage = env::storage_usage();

		let token_series_id = self.token_series_by_title.get(&token_series_title).expect("no series");
		let mut token_series = self.token_series_by_id.get(&token_series_id).expect("no token");
		assert_eq!(&env::predecessor_account_id(), &token_series.owner_id, "not series owner");

		let num_tokens = token_series.tokens.len();
		let max_copies = token_series.metadata.copies.unwrap_or(u64::MAX);
		assert_ne!(num_tokens, max_copies, "series supply maxed");

		let token_id = format!("{}{}{}", &token_series_id, TOKEN_DELIMETER, num_tokens + 1);
		token_series.tokens.insert(&token_id);
		self.token_series_by_id.insert(&token_series_id, &token_series);

		// you can add custom metadata to each token here
		// make sure you update self.nft_token to "patch" over the series metadata
		let metadata = Some(TokenMetadata {
			title: None, // ex. "Arch Nemesis: Mail Carrier" or "Parcel #5055"
			description: None, // free-form description
			media: None, // URL to associated media, preferably to decentralized, content-addressed storage
			copies: None, // number of copies of this set of metadata in existence when token was minted.
			media_hash: None, // Base64-encoded sha256 hash of content referenced by the `media` field. Required if `media` is included.
			issued_at: None, // ISO 8601 datetime when token was issued or minted
			expires_at: None, // ISO 8601 datetime when token expires
			starts_at: None, // ISO 8601 datetime when token starts being valid
			updated_at: None, // ISO 8601 datetime when token was last updated
			extra: None, // anything extra the NFT wants to store on-chain. Can be stringified JSON.
			reference: None, // URL to an off-chain JSON file with more info.
			reference_hash: None, // Base64-encoded sha256 hash of JSON from reference field. Required if `reference` is included.
		});
		let token = self.tokens.internal_mint(token_id.clone(), receiver_id.clone(), metadata);

        refund_deposit(env::storage_usage() - initial_storage_usage);

		env::log_str(format!("{}{}", EVENT_JSON, json!({
			"standard": "nep171",
			"version": "1.0.0",
			"event": "nft_mint",
			"data": [
			  	{
					  "owner_id": receiver_id,
					  "token_ids": [token_id]
				}
			]
		})).as_ref());
			
		token
	}

	/// CUSTOM re-implement core standard here, not using macros from near-contract-standards

	/// pass through
	#[payable]
	pub fn nft_transfer(
		&mut self,
		receiver_id: AccountId,
		token_id: TokenId,
		approval_id: Option<u64>,
		memo: Option<String>,
	) {
		self.tokens.nft_transfer(receiver_id, token_id, approval_id, memo)
	}

	/// pass through
	#[payable]
	pub fn nft_transfer_call(
		&mut self,
		receiver_id: AccountId,
		token_id: TokenId,
		approval_id: Option<u64>,
		memo: Option<String>,
		msg: String,
	) -> PromiseOrValue<bool> {
		self.tokens.nft_transfer_call(receiver_id, token_id, approval_id, memo, msg)
	}

	/// CUSTOM royalties payout
	#[payable]
	pub fn nft_transfer_payout(
		&mut self,
		receiver_id: AccountId,
		token_id: TokenId,
		approval_id: u64,
		memo: Option<String>,
		balance: Option<U128>,
		max_len_payout: Option<u32>,
	) -> Option<Payout> {

		// lazy minting?
		let series_mint_args = memo.clone();
		let previous_token = if let Some(series_mint_args) = series_mint_args {
			let SeriesMintArgs{token_series_title, receiver_id} = near_sdk::serde_json::from_str(&series_mint_args).expect("invalid SeriesMintArgs");
			self.nft_mint_series(token_series_title, receiver_id.clone())
		} else {
			let prev_token = self.nft_token(token_id.clone()).expect("no token");
			self.tokens.nft_transfer(receiver_id.clone(), token_id.clone(), Some(approval_id), memo);
			prev_token
		};

        // compute payouts based on balance option
        let owner_id = previous_token.owner_id;
        let payout_struct = if let Some(balance) = balance {
			let complete_royalty = 10_000u128;
            let balance_piece = u128::from(balance) / complete_royalty;
			let mut total_royalty_percentage = 0;
            let mut payout_struct: Payout = Payout{
				payout: HashMap::new()
			};
			let mut token_id_iter = token_id.split(TOKEN_DELIMETER);
			let token_series_id = token_id_iter.next().unwrap().parse().unwrap();
            let royalty = self.token_series_by_id.get(&token_series_id).expect("no series").royalty;

            if let Some(max_len_payout) = max_len_payout {
                assert!(royalty.len() as u32 <= max_len_payout, "exceeds max_len_payout");
            }
            for (k, v) in royalty.iter() {
                let key = k.clone();
				// skip seller and payout once at end
                if key != owner_id {
                    payout_struct.payout.insert(key, U128(*v as u128 * balance_piece));
                    total_royalty_percentage += *v;
                }
            }
            // payout to seller
            payout_struct.payout.insert(owner_id.clone(), U128((complete_royalty - total_royalty_percentage as u128) * balance_piece));
            Some(payout_struct)
        } else {
            None
        };

		env::log_str(format!("{}{}", EVENT_JSON, json!({
			"standard": "nep171",
			"version": "1.0.0",
			"event": "nft_transfer",
			"data": [
				{
					"old_owner_id": owner_id, "new_owner_id": receiver_id, "token_ids": [token_id]
				}
			]
		})).as_ref());

        payout_struct
	}

	/// CUSTOM re-implementation of near-contract-standards (not using macros)
	
	/// CUSTOM every enumeration method goes through here (watch the gas on views...)
	
	pub fn nft_token(&self, token_id: TokenId) -> Option<Token> {
		let owner_id = self.tokens.owner_by_id.get(&token_id)?;
        let approved_account_ids = self.tokens
            .approvals_by_id
			.as_ref()
            .and_then(|by_id| by_id.get(&token_id).or_else(|| Some(HashMap::new())));

		// CUSTOM (switch metadata for the token_series metadata)
		let mut token_id_iter = token_id.split(TOKEN_DELIMETER);
		let token_series_id = token_id_iter.next().unwrap().parse().unwrap();
		// make edition titles nice for showing in wallet
		let mut metadata = self.token_series_by_id.get(&token_series_id).unwrap().metadata;
		let copies = metadata.copies;
		if let Some(copies) = copies {
			metadata.title = Some(
				format!(
					"{}{}{}{}{}",
					metadata.title.unwrap(),
					TITLE_DELIMETER,
					token_id_iter.next().unwrap(),
					EDITION_DELIMETER,
					copies
				)
			);
		}
		
		// CUSTOM
		// implement this if you need to combine individual token metadata
		// e.g. metadata.extra with TokenSeries.metadata.extra and return something unique
		// let token_metadata = self.tokens.token_metadata_by_id.get(&token_id)?;
		// metadata.extra = token_metadata.extra;

        Some(Token { token_id, owner_id, metadata: Some(metadata), approved_account_ids })
	}

	pub fn nft_total_supply(&self) -> U128 {
		(self.tokens.owner_by_id.len() as u128).into()
	}

    pub fn nft_tokens(&self, from_index: Option<U128>, limit: Option<u64>) -> Vec<Token> {
        let start_index: u128 = from_index.map(From::from).unwrap_or_default();
        assert!(
            (self.tokens.owner_by_id.len() as u128) > start_index,
            "start_index gt len"
        );
        let limit = limit.map(|v| v as usize).unwrap_or(usize::MAX);
        assert_ne!(limit, 0, "Cannot provide limit of 0.");
        self.tokens.owner_by_id
            .iter()
            .skip(start_index as usize)
            .take(limit)
            .map(|(token_id, _)| self.nft_token(token_id).unwrap())
            .collect()
    }

    pub fn nft_supply_for_owner(self, account_id: AccountId) -> U128 {
        let tokens_per_owner = self.tokens.tokens_per_owner.expect(
            "Could not find tokens_per_owner when calling a method on the enumeration standard.",
        );
        tokens_per_owner
            .get(&account_id)
            .map(|account_tokens| U128::from(account_tokens.len() as u128))
            .unwrap_or(U128(0))
    }

	pub fn nft_tokens_for_owner(
        &self,
        account_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<Token> { 
		let tokens = self.tokens.tokens_per_owner.as_ref().expect("no tokens").get(&account_id).expect("no tokens");
		unordered_set_pagination(&tokens, from_index, limit)
			.iter()
			.map(|token_id| self.nft_token(token_id.clone()).unwrap())
            .collect()
    }

	/// CUSTOM VIEWS for seriesd tokens

	pub fn nft_get_series_json(&self, token_series_title: TokenSeriesTitle) -> TokenSeriesJson {
		let token_series = self.token_series_by_id.get(&self.token_series_by_title.get(&token_series_title).expect("no series")).expect("no series");
		TokenSeriesJson{
			metadata: token_series.metadata,
			owner_id: token_series.owner_id,
			royalty: token_series.royalty,
		}
	}

	pub fn nft_get_series(
		&self,
		from_index: Option<U128>,
		limit: Option<u64>
	) -> Vec<TokenSeriesJson> {
		unordered_map_val_pagination(&self.token_series_by_id, from_index, limit)
			.iter()
			.map(|token_series| TokenSeriesJson{
				metadata: token_series.metadata.clone(),
				owner_id: token_series.owner_id.clone(),
				royalty: token_series.royalty.clone(),
			})
            .collect()
    }

	pub fn nft_supply_for_series(
        &self,
        token_series_title: TokenSeriesTitle,
    ) -> U64 {
        self.token_series_by_id.get(&self.token_series_by_title.get(&token_series_title).expect("no series")).expect("no series").tokens.len().into()
    }

	pub fn nft_tokens_by_series(
		&self,
        token_series_title: TokenSeriesTitle,
		from_index: Option<U128>,
		limit: Option<u64>
	) -> Vec<Token> {
		let tokens = self.token_series_by_id.get(&self.token_series_by_title.get(&token_series_title).expect("no series")).expect("no series").tokens;
		unordered_set_pagination(&tokens, from_index, limit)
			.iter()
			.map(|token_id| self.nft_token(token_id.clone()).unwrap())
            .collect()
    }

	pub fn nft_get_series_format(&self) -> (char, &'static str, &'static str) {
		(TOKEN_DELIMETER, TITLE_DELIMETER, EDITION_DELIMETER)
	}
}

// near-contract-standards macros
// near_contract_standards::impl_non_fungible_token_core!(Contract, tokens);
// near_contract_standards::impl_non_fungible_token_enumeration!(Contract, tokens);
near_contract_standards::impl_non_fungible_token_approval!(Contract, tokens);

#[near_bindgen]
impl NonFungibleTokenMetadataProvider for Contract {
    fn nft_metadata(&self) -> NFTContractMetadata {
        self.metadata.get().unwrap()
    }
}

#[near_bindgen]
impl NonFungibleTokenResolver for Contract {
	#[private]
	fn nft_resolve_transfer(
		&mut self,
		previous_owner_id: AccountId,
		receiver_id: AccountId,
		token_id: TokenId,
		approved_account_ids: Option<HashMap<AccountId, u64>>,
	) -> bool {
		self.tokens.nft_resolve_transfer(
			previous_owner_id,
			receiver_id,
			token_id,
			approved_account_ids,
		)
	}
}