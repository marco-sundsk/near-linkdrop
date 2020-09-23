use borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::collections::Map;
use near_sdk::json_types::{Base58PublicKey, U128};
use near_sdk::{
    env, ext_contract, near_bindgen, AccountId, Balance, Promise, PublicKey,
};
use std::convert::TryInto;

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

/// 红包信息结构
#[derive(Clone)]
#[derive(BorshDeserialize, BorshSerialize)]
pub struct RedInfo {
    pub mode: u8, // 红包模式,随机红包1;均分红包0
    pub count: u128, // 红包数量
    pub slogan: String, // 口号
    pub balance: Balance, // 总金额
    pub remaining_balance: u128, // 红包剩余金额
}

#[derive(Clone)]
#[derive(BorshDeserialize, BorshSerialize)]
pub struct ReceivedRedInfo {
    pub amount: Balance, // 领取到红包价值

    pub redbag: Base58PublicKey, // 红包
}

pub type RedInfoKey = Vec<u8>;

#[near_bindgen]
#[derive(Default, BorshDeserialize, BorshSerialize)]
pub struct LinkDrop {
    pub accounts: Map<PublicKey, Balance>,

    pub red_info: Map<PublicKey, RedInfo>, // 发送红包信息，key为随机信息，value为红包信息

    pub sender_redbag: Map<AccountId, Vec<Base58PublicKey>>, // 发送红包用户与红包关联关系

    pub red_receive_record: Map<PublicKey, Vec<AccountId>>, // 红包领取记录(某红包被哪些人领取)

    pub red_receive_detail: Map<(PublicKey, AccountId), u128>, // 红包领取详细信息（红包、领取人、领取数量）

    pub receiver_redbag_record: Map<AccountId, Vec<ReceivedRedInfo>>, // 用户所领取的红包
}

/// Access key allowance for linkdrop keys.
const ACCESS_KEY_ALLOWANCE: u128 = 1_000_000_000_000_000_000_000_000;

/// Gas attached to the callback from account creation.
pub const ON_CREATE_ACCOUNT_CALLBACK_GAS: u64 = 20_000_000_000_000;

/// Indicates there are no deposit for a callback for better readability.
const NO_DEPOSIT: u128 = 0;

#[ext_contract(ext_self)]
pub trait ExtLinkDrop {
    /// Callback after plain account creation.
    fn on_account_created(&mut self, predecessor_account_id: AccountId, amount: U128) -> bool;

    /// Callback after creating account and claiming linkdrop.
    fn on_account_created_and_claimed(&mut self, amount: U128) -> bool;
}

#[near_bindgen]
impl LinkDrop {

    ///  发（创建）红包功能
    #[payable]
    pub fn send_redbag(&mut self, public_key: Base58PublicKey, count: u128, mode: u8, slogan: String) -> Promise {
        assert!(
            env::attached_deposit() > ACCESS_KEY_ALLOWANCE,
            "Attached deposit must be greater than ACCESS_KEY_ALLOWANCE"
        );

        let pk = public_key.clone().into();

        // 红包信息
        let new_red_info = RedInfo {
            mode: mode,
            count: count,
            slogan: slogan,
            balance: env::attached_deposit(),
            remaining_balance: env::attached_deposit(),
        };

        assert!(self.red_info.get(&pk).is_none(), "existed");

        self.red_info.insert(&pk, &new_red_info);
        let mut relation_vec = self.sender_redbag.get(&env::signer_account_id()).unwrap_or(Vec::new());
        relation_vec.push(public_key.clone());
        self.sender_redbag.insert(&env::signer_account_id(), &relation_vec);

        Promise::new(env::current_account_id()).add_access_key(
            pk,
            ACCESS_KEY_ALLOWANCE,
            env::current_account_id(),
            b"create_account_and_claim,claim,revoke".to_vec(),
        )
    }

    /// 创建新用户并同时领取红包
    pub fn create_account_and_claim(
        &mut self,
        new_account_id: AccountId,
        new_public_key: Base58PublicKey) -> Promise {

        let pk = env::signer_account_pk();

        // 查看红包是否存在
        let redbag = self.red_info.get(&pk);
        assert!(redbag.is_some(), "红包不存在");

        // 查看红包剩余数量是否可被领取
        let temp_redbag = &redbag.unwrap();
        let count = temp_redbag.count;
        let remaining_balance = temp_redbag.remaining_balance;

        let mut record = self.red_receive_record.get(&pk).unwrap_or(Vec::new());
        assert!(record.len() < count.try_into().unwrap(), "红包已被领取完");

        record.push(String::from(&new_account_id));
        self.red_receive_record.insert(&pk, &record);

        self.red_receive_detail.insert(&(pk.clone().into(), new_account_id.clone()), &count);

        // 分配红包
        let mut receiver_record = self.receiver_redbag_record.get(&new_account_id).unwrap_or(Vec::new());

        let amount: Balance = self.random_amount(remaining_balance);

        let received_redbag_info = ReceivedRedInfo {
            amount: amount,
            redbag: Base58PublicKey(pk.clone().into()),
        };

        receiver_record.push(received_redbag_info);
        self.receiver_redbag_record.insert(&new_account_id, &receiver_record);

        let new_red_info = RedInfo {
            mode: temp_redbag.clone().mode,
            count: temp_redbag.clone().count,
            slogan: temp_redbag.clone().slogan,
            balance: temp_redbag.clone().balance,
            remaining_balance: temp_redbag.clone().remaining_balance - amount,
        };

        self.red_info.insert(&pk, &new_red_info);

        Promise::new(new_account_id)
            .create_account()
            .add_full_access_key(new_public_key.into())
            .transfer(amount)
            .then(ext_self::on_account_created(
                env::predecessor_account_id(),
                amount.into(),
                &env::current_account_id(),
                NO_DEPOSIT,
                ON_CREATE_ACCOUNT_CALLBACK_GAS,
            ))
    }

    /// 领取红包
    pub fn claim(&mut self, account_id: AccountId) -> Promise {
        let pk = env::signer_account_pk();

        // 查看红包是否存在
        let redbag = self.red_info.get(&pk);
        assert!(redbag.is_some(), "红包不存在");

        // 查看红包剩余数量是否可被领取
        let temp_redbag = &redbag.unwrap();
        let count = temp_redbag.count;
        let remaining_balance = temp_redbag.remaining_balance;
        let mut record = self.red_receive_record.get(&pk).unwrap_or(Vec::new());
        assert!(record.len() < count.try_into().unwrap(), "红包已被领取完");

        // 判断用户手否领取过
        for x in &record {
            assert!(String::from(x) != account_id, "该用户已领取过");
        }

        record.push(String::from(&account_id));
        self.red_receive_record.insert(&pk, &record);
        self.red_receive_detail.insert(&(pk.clone().into(), account_id.clone()), &count);

        // 分配红包
        let mut receiver_record = self.receiver_redbag_record.get(&account_id).unwrap_or(Vec::new());

        let amount: Balance = self.random_amount(remaining_balance);

        let received_redbag_info = ReceivedRedInfo {
            amount: amount,
            redbag: Base58PublicKey(pk.clone().into()),
        };

        receiver_record.push(received_redbag_info);
        self.receiver_redbag_record.insert(&account_id, &receiver_record);

        let new_red_info = RedInfo {
            mode: temp_redbag.clone().mode,
            count: temp_redbag.clone().count,
            slogan: temp_redbag.clone().slogan,
            balance: temp_redbag.clone().balance,
            remaining_balance: temp_redbag.clone().remaining_balance - amount,
        };

        self.red_info.insert(&pk, &new_red_info);

        // 减少红包数量及金额
        Promise::new(account_id).transfer(amount)
    }

    /// 发红包任用来撤回对应public_key的红包剩余金额
    pub fn revoke(&mut self, public_key: Base58PublicKey) -> &str {
        let pk = public_key.clone().into();
        self.red_info.remove(&pk);
        let mut red_list = self.sender_redbag.get(&env::signer_account_id()).unwrap();

        let mut index = 0;
        for item in red_list.clone().iter() {
            if item == &public_key {
                break;
            }
            index += 1;
        }

        red_list.remove(index);
        self.sender_redbag.insert(&env::signer_account_id(), &red_list);
        "revoke success"
    }

    /// 查询用户发的红包
    pub fn show_claim_info(self, public_key: Base58PublicKey) -> String {
        let pk = public_key.into();
        let red_info_obj = self.red_info.get(&pk);

        assert!(red_info_obj.is_some(), "红包不存在");

        let receive_record = self.red_receive_record.get(&pk).unwrap_or(Vec::new());

        let mut record_list = String::from("[");
        for item in receive_record.iter() {
            let amount = self.red_receive_detail.get(&(pk.clone().into(), String::from(item))).unwrap();
            record_list.push_str(&format!("{}\"account\":\"{}\", \"amount\":{}{},", "{", item, amount, "}"));
        }
        record_list.push_str("]");

        let temp_red_info = red_info_obj.unwrap();
        format!("{}\"count\":{}, \"mode\":{}, \"slogan\":\"{}\",\"list\":\"{}\"{}", "{", temp_red_info.count, temp_red_info.mode, temp_red_info.slogan, record_list, "}")
    }

    /// 查询用户所发的所有红包
    pub fn show_redbag(self, account_id: AccountId) -> Vec<Base58PublicKey> {
        let relation_vec = self.sender_redbag.get(&account_id).unwrap_or(Vec::new());
        relation_vec
    }

    /// 生成随机
    fn random_amount(&self, total_amount: u128) -> u128 {
        let u8_max_value: u128 = u8::max_value().into();
        let block_length = total_amount / u8_max_value;

        let random_seed = env::random_seed();

        // 计算总 seed 值
        let mut block_index = 0_u8;

        for item in random_seed {
            block_index = block_index.wrapping_add(item);
        }

        // TODO 有待检查
        if block_index < 1 {
            block_index += 1;
        } else if block_index > 253 {
            block_index -= 1;
        }

        block_length.wrapping_mul(block_index.into())
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
mod tests {
    use std::convert::TryInto;

    use near_sdk::MockedBlockchain;
    use near_sdk::{testing_env, BlockHeight, PublicKey, VMContext};
}
