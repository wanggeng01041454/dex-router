use anchor_lang::prelude::*;

pub const POOL_VERSION_CLMM: u8 = 6; // Constant for CLMM pool version
pub const POOL_VERSION_CPMM: u8 = 7; // Constant for CPMM pool version

#[derive(Debug, Clone)]
pub struct BasicPoolInfo {
    pub id: Pubkey,        // Unique identifier for the pool
    pub version: u8,      // Version of the pool (6 for CLMM, 7 for CPMM)
    pub mint_a: Pubkey,    // Token A's mint address
    pub mint_b: Pubkey,    // Token B's mint address
}

// todo: poolInfo缓存在本地， 新增时，链解析后端要推送
