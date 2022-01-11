use crate::*;
use near_sdk::{log, promise_result_as_success};

/// measuring how many royalties can be paid
const GAS_FOR_FT_TRANSFER: Gas = Gas(5_000_000_000_000);
/// seems to be max Tgas can attach to resolve_purchase
const GAS_FOR_ROYALTIES: Gas = Gas(120_000_000_000_000);
const GAS_FOR_NFT_TRANSFER: Gas = Gas(20_000_000_000_000);

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Bid {
    pub owner_id: AccountId,
    pub price: U128,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Sale {
    pub owner_id: AccountId,
    pub approval_id: u64,
    pub nft_contract_id: AccountId,
    pub token_id: String,
    pub conditions: HashMap<FungibleTokenId, U128>,
    pub created_at: U64,
    pub is_series: Option<bool>,
    pub token_type: Option<String>,
    pub bids: Option<HashMap<FungibleTokenId, Bid>>,
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Price {
    pub ft_token_id: AccountId,
    pub price: Option<U128>,
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct PurchaseArgs {
    pub nft_contract_id: AccountId,
    pub token_id: TokenId,
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct SeriesMintArgs {
    pub series_name: String,
    pub mint: Vec<String>,
    pub owner: Vec<String>,
    pub perpetual_royalties: Option<HashMap<AccountId, u32>>,
    pub receiver_id: Option<AccountId>,
}

#[near_bindgen]
impl Contract {
    /// for add sale see: nft_callbacks.rs
    #[payable]
    pub fn remove_sale(&mut self, nft_contract_id: AccountId, token_id: String) {
        assert_one_yocto();
        let sale = self.internal_remove_sale(nft_contract_id.into(), token_id);
        let owner_id = env::predecessor_account_id();
        assert_eq!(owner_id, sale.owner_id, "Must be sale owner");
        self.refund_bids(sale.bids.unwrap_or_default());
    }

    #[payable]
    pub fn update_price(
        &mut self,
        nft_contract_id: AccountId,
        token_id: String,
        ft_token_id: AccountId,
        price: U128,
    ) {
        assert_one_yocto();
        let contract_id: AccountId = nft_contract_id.into();
        let contract_and_token_id = format!("{}{}{}", contract_id, DELIMETER, token_id);
        let mut sale = self.sales.get(&contract_and_token_id).expect("No sale");
        assert_eq!(
            env::predecessor_account_id(),
            sale.owner_id,
            "Must be sale owner"
        );
        if !self.ft_token_ids.contains(&ft_token_id) {
            env::panic_str(&format!("Token {} not supported by this market", ft_token_id));
        }
        sale.conditions.insert(ft_token_id.into(), price);
        self.sales.insert(&contract_and_token_id, &sale);
    }

    #[payable]
    pub fn offer(
        &mut self,
        nft_contract_id: AccountId,
        token_id: String,
        msg: Option<String>,
    ) {
        let contract_id: AccountId = nft_contract_id;
        let contract_and_token_id = format!("{}{}{}", contract_id, DELIMETER, token_id);
        let sale = self.sales.get(&contract_and_token_id).expect("No sale");
        let buyer_id = env::predecessor_account_id();
        if sale.is_series.is_none() {
            assert_ne!(sale.owner_id, buyer_id, "Cannot bid on your own sale.");
        }
        let price = sale
            .conditions
            .get(&self.near_ft)
            .expect("Not for sale in NEAR")
            .0;

        let deposit = env::attached_deposit();
        let msg_is_some = msg.is_some();
        assert!(deposit > 0, "Attached deposit must be greater than 0");
        // there's a fixed price user can buy for so process purchase
        // or, with memo user is passing through their deposit
        if deposit == price || msg_is_some {
            let diff = deposit.checked_sub(price).expect("Attached deposit for minting is less than price.");
            if msg_is_some {
                assert!(diff > 0, "Attached deposit must be greater than price (to pay for storage of minted NFT).");
            }
            self.process_purchase(
                sale,
                contract_id,
                token_id,
                self.near_ft.clone(),
                msg,
                U128(deposit),
                U128(price),
                buyer_id,
            );
        } else {
            self.add_bid(contract_and_token_id, price, deposit, self.near_ft.clone(), buyer_id)
        }
    }

    #[private]
    pub fn process_purchase(
        &mut self,
        sale: Sale,
        nft_contract_id: AccountId,
        token_id: String,
        ft_token_id: AccountId,
        msg: Option<String>,
        paid: U128,
        price: U128,
        buyer_id: AccountId,
    ) -> Promise {
        if sale.is_series.is_none() {
            self.internal_remove_sale(nft_contract_id.clone(), token_id.clone());
        }

        let mut nft_transfer_deposit = paid.0.saturating_sub(price.0);
        if nft_transfer_deposit < 1 {
            nft_transfer_deposit = 1
        }

        ext_contract::nft_transfer_payout(
            buyer_id.clone(),
            token_id,
            sale.approval_id,
            msg,
            price,
            nft_contract_id,
            // price paid remains with contract (excess deposit for storage cost of series lazy mint)
            nft_transfer_deposit,
            GAS_FOR_NFT_TRANSFER,
        )
        .then(ext_self::resolve_purchase(
            ft_token_id,
            buyer_id,
            sale,
            paid,
            env::current_account_id(),
            NO_DEPOSIT,
            GAS_FOR_ROYALTIES,
        ))
    }

    /// self callback

    #[private]
    pub fn resolve_purchase(
        &mut self,
        ft_token_id: AccountId,
        buyer_id: AccountId,
        sale: Sale,
        paid: U128,
    ) -> U128 {
        let bids = sale.bids.unwrap_or_default();
        let price = sale.conditions[&ft_token_id];

        // checking for payout information
        let payout_option = promise_result_as_success().and_then(|value| {
            // None means a bad payout from bad NFT contract
            near_sdk::serde_json::from_slice::<Payout>(&value)
                .ok()
                .and_then(|payout_struct| {
                    // gas to do 10 FT transfers (and definitely 10 NEAR transfers)
                    if payout_struct.payout.len() + bids.len() > 10 || payout_struct.payout.is_empty() {
                        log!("Cannot have more than 10 royalties and sale.bids refunds");
                        None
                    } else {
                        // TODO off by 1 e.g. payouts are fractions of 3333 + 3333 + 3333
                        let mut remainder = price.0;
                        for &value in payout_struct.payout.values() {
                            remainder = remainder.checked_sub(value.0)?;
                        }
                        if remainder == 0 || remainder == 1 {
                            Some(payout_struct)
                        } else {
                            None
                        }
                    }
                })
        });
        // is payout option valid?
        let payout_struct = if let Some(payout_option) = payout_option {
            payout_option
        } else {
            if ft_token_id == self.near_ft {
                // TODO pay back the deposit for minting the token if this was a series purchase and unsuccessful
                Promise::new(buyer_id).transfer(u128::from(paid));
            }
            // leave function and return all FTs in ft_resolve_transfer
            return paid;
        };
        // Goint to payout everyone, first return all outstanding bids (accepted offer bid was already removed)
        self.refund_bids(bids);

        // NEAR payouts
        if ft_token_id == self.near_ft.clone() {
            for (receiver_id, amount) in payout_struct.payout {
                Promise::new(receiver_id).transfer(amount.0);
            }
            // refund all FTs (won't be any)
            price
        } else {
            // FT payouts
            for (receiver_id, amount) in payout_struct.payout {
                ext_contract::ft_transfer(
                    receiver_id,
                    amount,
                    None,
                    ft_token_id.clone(),
                    1,
                    GAS_FOR_FT_TRANSFER,
                );
            }
            // keep all FTs (already transferred for payouts)
            U128(0)
        }
    }

    #[private]
    pub fn add_bid(
        &mut self,
        contract_and_token_id: ContractAndTokenId,
        price: Balance,
        amount: Balance,
        ft_token_id: AccountId,
        buyer_id: AccountId,
    ) {
        assert!(
            price == 0 || amount < price,
            "Paid more {} than price {}",
            amount,
            price
        );
        // store a bid and refund any current bid lower
        let new_bid = Bid {
            owner_id: buyer_id,
            price: U128(amount),
        };
        let mut sale = self.sales.get(&contract_and_token_id).expect("No sale");
        let mut bids = sale.bids.unwrap_or_default();
        let current_bid = bids.get(&ft_token_id);
        if let Some(current_bid) = current_bid {
            // refund current bid holder
            let current_price: u128 = current_bid.price.into();
            assert!(
                amount > current_price,
                "Can't pay less than or equal to current bid price: {}",
                current_price
            );
            Promise::new(current_bid.owner_id.clone()).transfer(current_bid.price.into());
            bids.insert(ft_token_id, new_bid);
        } else {
            bids.insert(ft_token_id, new_bid);
        }
        sale.bids = Some(bids);
        self.sales.insert(&contract_and_token_id, &sale);
    }

    #[payable]
    pub fn accept_offer(
        &mut self,
        nft_contract_id: AccountId,
        token_id: String,
        ft_token_id: AccountId,
    ) {
        assert_one_yocto();

        let contract_id: AccountId = nft_contract_id.into();
        let contract_and_token_id = format!("{}{}{}", contract_id, DELIMETER, token_id);
        // remove bid before proceeding to process purchase
        let mut sale = self.sales.get(&contract_and_token_id).expect("No sale");
        let mut bids = sale.bids.unwrap_or_default();
        let bid = bids.remove(&ft_token_id).expect("No bid");
        sale.bids = Some(bids);
        self.sales.insert(&contract_and_token_id, &sale);
        // panics at `self.internal_remove_sale` and reverts above if predecessor is not sale.owner_id
        self.process_purchase(
            sale,
            contract_id,
            token_id,
            ft_token_id.into(),
            None,
            bid.price,
            bid.price,
            bid.owner_id,
        );
    }

    /// internal

    fn refund_bids(&mut self, bids: HashMap<FungibleTokenId, Bid>) {
        for (bid_ft, bid) in bids {
            if bid_ft == self.near_ft {
                Promise::new(bid.owner_id.clone()).transfer(u128::from(bid.price));
            } else {
                ext_contract::ft_transfer(
                    bid.owner_id.clone(),
                    bid.price,
                    None,
                    bid_ft,
                    1,
                    GAS_FOR_FT_TRANSFER,
                );
            }
        }
    }
}

/// self call

#[ext_contract(ext_self)]
trait ExtSelf {
    fn resolve_purchase(
        &mut self,
        ft_token_id: AccountId,
        buyer_id: AccountId,
        sale: Sale,
        paid: U128,
    ) -> Promise;
}
