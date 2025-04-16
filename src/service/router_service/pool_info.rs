use std::{
  collections::VecDeque,
  ops::{DerefMut, Neg},
};

use anyhow::{Result, anyhow};
use raydium_amm_v3::{
  libraries::{TICK_ARRAY_BITMAP_SIZE, U512, U1024, liquidity_math, swap_math, tick_array_bit_map, tick_math},
  states::{
    POOL_TICK_ARRAY_BITMAP_SEED, TICK_ARRAY_SEED, TICK_ARRAY_SIZE, TickArrayBitmapExtension, TickArrayState, TickState, tick_array,
  },
};
use solana_sdk::pubkey::Pubkey;

use crate::constants::BYREAL_CLMM_PROGRAM_ID;

use super::types::PoolInfo;

const EXTENSION_TICKARRAY_BITMAP_SIZE: usize = 14;

// the top level state of the swap, the results of which are recorded in storage at the end
#[derive(Debug)]
pub struct SwapState {
  // the amount remaining to be swapped in/out of the input/output asset
  pub amount_specified_remaining: u64,
  // the amount already swapped out/in of the output/input asset
  pub amount_calculated: u64,
  // current sqrt(price)
  pub sqrt_price_x64: u128,
  // the tick associated with the current price
  pub tick: i32,
  // the current liquidity in range
  pub liquidity: u128,
  // swap过程中，fee 的累加值
  pub fee_amount: u64,
}

#[derive(Default)]
struct StepComputations {
  // the price at the beginning of the step
  sqrt_price_start_x64: u128,
  // the next tick to swap to from the current tick in the swap direction
  tick_next: i32,
  // whether tick_next is initialized or not
  initialized: bool,
  // sqrt(price) for the next tick (1/0)
  sqrt_price_next_x64: u128,
  // how much is being swapped in in this step
  amount_in: u64,
  // how much is being swapped out
  amount_out: u64,
  // how much fee is being paid in
  fee_amount: u64,
}

impl PoolInfo {
  pub fn tick_array_bitmap_extension_key(pool_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[POOL_TICK_ARRAY_BITMAP_SEED.as_bytes(), pool_id.as_ref()], &BYREAL_CLMM_PROGRAM_ID).0
  }
  /// 一个 tickArray 账户的 tick_count
  pub fn get_tick_count_in_tick_array(tick_spacing: u16) -> i32 {
    TICK_ARRAY_SIZE * i32::from(tick_spacing)
  }

  /// 一个 tickArray-bitmap 中的最大 tick 数
  pub fn get_max_tick_in_tickarray_bitmap(tick_spacing: u16) -> i32 {
    i32::from(tick_spacing) * TICK_ARRAY_SIZE * TICK_ARRAY_BITMAP_SIZE
  }

  pub fn calculate_all_tick_array_keys(
    pool_id: &Pubkey,
    tick_spacing: u16,
    tick_array_bitmap: &[u64; 16],
    tick_array_bitmap_extension: &TickArrayBitmapExtension,
  ) -> Vec<Pubkey> {
    let tick_count_in_tickarray_bitmap = Self::get_max_tick_in_tickarray_bitmap(tick_spacing);
    let tick_count_in_tick_array = Self::get_tick_count_in_tick_array(tick_spacing);

    let mut tick_array_keys = Vec::new();
    // 扩展的左侧tick； tick_index < 0
    // offset 代表 bitmap 的序号，从最大的开始， 最大的离的最远（对应的tick_index的绝对值越大）
    for offset in (0..EXTENSION_TICKARRAY_BITMAP_SIZE).rev() {
      let tick_array_bitmap = U512(tick_array_bitmap_extension.negative_tick_array_bitmap[offset]);

      // 在当前的bitmap中，tick_index 的基数
      let base_tick_index = -1 * (offset + 1) as i32 * tick_count_in_tickarray_bitmap;

      for bitmap_inner_index in 0..TICK_ARRAY_BITMAP_SIZE {
        if tick_array_bitmap.bit(bitmap_inner_index as usize) {
          // 对应 raydium-clmm 源码中的这段代码：
          // let m = tick_array_start_index.abs() % max_tick_in_tickarray_bitmap(tick_spacing);
          // let mut tick_array_offset_in_bitmap = m / TickArrayState::tick_count(tick_spacing);
          // if tick_array_start_index < 0 && m != 0 {
          //     tick_array_offset_in_bitmap = TICK_ARRAY_BITMAP_SIZE - tick_array_offset_in_bitmap;
          // }

          let real_bitmap_inner_index =
            if bitmap_inner_index != 0 { TICK_ARRAY_BITMAP_SIZE - bitmap_inner_index } else { bitmap_inner_index };
          let mut start_tick_index = base_tick_index - real_bitmap_inner_index * tick_count_in_tick_array;

          // 这里是对应
          // if tick_index < 0 && tick_index.abs() % ticks_in_one_bitmap == 0 {
          //   offset -= 1;
          // }
          if bitmap_inner_index == 0 {
            start_tick_index -= tick_count_in_tickarray_bitmap;
          }

          // 上面这两段代码合起来的逻辑，等价于：
          // let start_tick_index = base_tick_index - (TICK_ARRAY_BITMAP_SIZE - bitmap_inner_index) * tick_count_in_tick_array;

          println!(
            "extension negative offset:{}, bitmap_inner_index:{}, real_bitmap_inner_index:{}, start_tick_index:{}",
            offset, bitmap_inner_index, real_bitmap_inner_index, start_tick_index
          );
          let tick_array_account_address = Self::get_pda_tick_array_address(pool_id, start_tick_index);
          tick_array_keys.push(tick_array_account_address);
        }
      }
    }

    let tick_array_bitmap = U1024(*tick_array_bitmap);
    //左侧tick
    for i in 0..TICK_ARRAY_BITMAP_SIZE {
      if tick_array_bitmap.bit(i as usize) {
        // 这个bitmap对应的 tick_array_account 的 start_tick_index
        let start_tick_index = (i - TICK_ARRAY_BITMAP_SIZE) * tick_count_in_tick_array;
        println!("left tick, bitmap_index: {}, start_tick_index: {}", i, start_tick_index);

        let tick_array_account_address = Self::get_pda_tick_array_address(pool_id, start_tick_index);
        tick_array_keys.push(tick_array_account_address);
      }
    }

    //右侧tick
    for i in 0..TICK_ARRAY_BITMAP_SIZE {
      if tick_array_bitmap.bit((i + TICK_ARRAY_BITMAP_SIZE) as usize) {
        // 这个bitmap对应的 tick_array_account 的 start_tick_index
        let start_tick_index = i * tick_count_in_tick_array;
        println!("right tick, bitmap_index: {}, start_tick_index: {}", i, start_tick_index);

        let tick_array_account_address = Self::get_pda_tick_array_address(pool_id, start_tick_index);
        tick_array_keys.push(tick_array_account_address);
      }
    }

    //扩展的右侧tick
    for offset in 0..EXTENSION_TICKARRAY_BITMAP_SIZE {
      let tick_array_bitmap = U512(tick_array_bitmap_extension.positive_tick_array_bitmap[offset]);

      // 在当前的bitmap中，tick_index 的基数
      let base_tick_index = (offset + 1) as i32 * tick_count_in_tickarray_bitmap;

      for bitmap_inner_index in 0..TICK_ARRAY_BITMAP_SIZE {
        if tick_array_bitmap.bit(bitmap_inner_index as usize) {
          let start_tick_index = base_tick_index + bitmap_inner_index * tick_count_in_tick_array;

          println!(
            "extension postive offset:{}, bitmap_inner_index:{}, bitmap_inner_index:{}, start_tick_index:{}",
            offset, bitmap_inner_index, bitmap_inner_index, start_tick_index
          );
          let tick_array_account_address = Self::get_pda_tick_array_address(pool_id, start_tick_index);
          tick_array_keys.push(tick_array_account_address);
        }
      }
    }

    tick_array_keys
  }

  //todo: 将需要的参数尽量存在 pool_info 中
  //todo: swap_compute 还需要返回的结果：输入代币还剩余多少，swap fee 用了多少， swap后的价格是多少
  /// 计算 swap 的结果
  pub fn swap_compute(
    &self,
    zero_for_one: bool,
    is_base_input: bool,
    is_pool_current_tick_array: bool,
    amount_specified: u64,
    current_vaild_tick_array_start_index: i32,
    sqrt_price_limit_x64: u128,
    tickarray_bitmap_extension: &TickArrayBitmapExtension,
    tick_arrays: &mut VecDeque<TickArrayState>,
  ) -> Result<(SwapState, VecDeque<i32>)> {
    if amount_specified == 0 {
      return Err(anyhow!("amountSpecified must not be 0"));
    }
    let sqrt_price_limit_x64 = if sqrt_price_limit_x64 == 0 {
      if zero_for_one { tick_math::MIN_SQRT_PRICE_X64 + 1 } else { tick_math::MAX_SQRT_PRICE_X64 - 1 }
    } else {
      sqrt_price_limit_x64
    };
    if zero_for_one {
      if sqrt_price_limit_x64 < tick_math::MIN_SQRT_PRICE_X64 {
        return Err(anyhow!("sqrt_price_limit_x64 must greater than MIN_SQRT_PRICE_X64"));
      }
      if sqrt_price_limit_x64 >= self.dynamic_info.sqrt_price_x64 {
        return Err(anyhow!("sqrt_price_limit_x64 must smaller than current"));
      }
    } else {
      if sqrt_price_limit_x64 > tick_math::MAX_SQRT_PRICE_X64 {
        return Err(anyhow!("sqrt_price_limit_x64 must less than MAX_SQRT_PRICE_X64"));
      }
      if sqrt_price_limit_x64 <= self.dynamic_info.sqrt_price_x64 {
        return Err(anyhow!("sqrt_price_limit_x64 must greater than current"));
      }
    }
    let mut tick_match_current_tick_array = is_pool_current_tick_array;

    let mut state = SwapState {
      amount_specified_remaining: amount_specified,
      amount_calculated: 0,
      sqrt_price_x64: self.dynamic_info.sqrt_price_x64,
      tick: self.dynamic_info.tick_current,
      liquidity: self.dynamic_info.liquidity,
      fee_amount: 0,
    };

    let mut tick_array_current = tick_arrays.pop_front().unwrap();
    if tick_array_current.start_tick_index != current_vaild_tick_array_start_index {
      return Err(anyhow!("tick array start tick index does not match"));
    }
    let mut tick_array_start_index_vec = VecDeque::new();
    tick_array_start_index_vec.push_back(tick_array_current.start_tick_index);
    let mut loop_count = 0;
    // loop across ticks until input liquidity is consumed, or the limit price is reached
    while state.amount_specified_remaining != 0
      && state.sqrt_price_x64 != sqrt_price_limit_x64
      && state.tick < tick_math::MAX_TICK
      && state.tick > tick_math::MIN_TICK
    {
      //todo, 移除这个限制 或者 调整这个限制
      if loop_count > 10 {
        return Err(anyhow!("loop count exceed limit"));
      }
      let mut step = StepComputations::default();
      step.sqrt_price_start_x64 = state.sqrt_price_x64;
      // save the bitmap, and the tick account if it is initialized
      let mut next_initialized_tick = if let Some(tick_state) =
        tick_array_current.next_initialized_tick(state.tick, self.base_info.tick_spacing, zero_for_one).unwrap()
      {
        Box::new(*tick_state)
      } else {
        // todo, 这个逻辑需要仔细考量
        if !tick_match_current_tick_array {
          tick_match_current_tick_array = true;
          Box::new(*tick_array_current.first_initialized_tick(zero_for_one).unwrap())
        } else {
          Box::new(TickState::default())
        }
      };
      if !next_initialized_tick.is_initialized() {
        let current_vaild_tick_array_start_index = self
          .next_initialized_tick_array_start_index(tickarray_bitmap_extension, current_vaild_tick_array_start_index, zero_for_one)
          .unwrap();
        tick_array_current = tick_arrays.pop_front().unwrap();
        if current_vaild_tick_array_start_index.is_none() {
          return Err(anyhow!("tick array start tick index out of range limit"));
        }
        if tick_array_current.start_tick_index != current_vaild_tick_array_start_index.unwrap() {
          return Err(anyhow!("tick array start tick index does not match"));
        }
        tick_array_start_index_vec.push_back(tick_array_current.start_tick_index);
        let mut first_initialized_tick = tick_array_current.first_initialized_tick(zero_for_one).unwrap();

        next_initialized_tick = Box::new(*first_initialized_tick.deref_mut());
      }
      step.tick_next = next_initialized_tick.tick;
      step.initialized = next_initialized_tick.is_initialized();
      if step.tick_next < tick_math::MIN_TICK {
        step.tick_next = tick_math::MIN_TICK;
      } else if step.tick_next > tick_math::MAX_TICK {
        step.tick_next = tick_math::MAX_TICK;
      }

      step.sqrt_price_next_x64 = tick_math::get_sqrt_price_at_tick(step.tick_next).unwrap();

      let target_price = if (zero_for_one && step.sqrt_price_next_x64 < sqrt_price_limit_x64)
        || (!zero_for_one && step.sqrt_price_next_x64 > sqrt_price_limit_x64)
      {
        sqrt_price_limit_x64
      } else {
        step.sqrt_price_next_x64
      };
      let swap_step = swap_math::compute_swap_step(
        state.sqrt_price_x64,
        target_price,
        state.liquidity,
        state.amount_specified_remaining,
        self.base_info.trade_fee_rate,
        is_base_input,
        zero_for_one,
        1,
      )
      .unwrap();
      state.sqrt_price_x64 = swap_step.sqrt_price_next_x64;
      state.fee_amount += swap_step.fee_amount;

      step.amount_in = swap_step.amount_in;
      step.amount_out = swap_step.amount_out;
      step.fee_amount = swap_step.fee_amount;

      if is_base_input {
        state.amount_specified_remaining = state.amount_specified_remaining.checked_sub(step.amount_in + step.fee_amount).unwrap();
        state.amount_calculated = state.amount_calculated.checked_add(step.amount_out).unwrap();
      } else {
        state.amount_specified_remaining = state.amount_specified_remaining.checked_sub(step.amount_out).unwrap();
        state.amount_calculated = state.amount_calculated.checked_add(step.amount_in + step.fee_amount).unwrap();
      }

      if state.sqrt_price_x64 == step.sqrt_price_next_x64 {
        // if the tick is initialized, run the tick transition
        if step.initialized {
          let mut liquidity_net = next_initialized_tick.liquidity_net;
          if zero_for_one {
            liquidity_net = liquidity_net.neg();
          }
          state.liquidity = liquidity_math::add_delta(state.liquidity, liquidity_net).unwrap();
        }

        state.tick = if zero_for_one { step.tick_next - 1 } else { step.tick_next };
      } else if state.sqrt_price_x64 != step.sqrt_price_start_x64 {
        // recompute unless we're on a lower tick boundary (i.e. already transitioned ticks), and haven't moved
        state.tick = tick_math::get_tick_at_sqrt_price(state.sqrt_price_x64).unwrap();
      }
      loop_count += 1;
    }

    Ok((state, tick_array_start_index_vec))
  }

  pub fn get_pda_tick_array_address(pool_id: &Pubkey, tick_array_start_index: i32) -> Pubkey {
    Pubkey::find_program_address(
      &[TICK_ARRAY_SEED.as_bytes(), pool_id.as_ref(), &tick_array_start_index.to_be_bytes()],
      &BYREAL_CLMM_PROGRAM_ID,
    )
    .0
  }

  pub fn get_tick_array_dequeue(&self, first_tick_array_start_index: i32, zero_for_one: bool) -> Result<VecDeque<TickArrayState>> {
    let mut tick_array_dequeue = VecDeque::new();

    if zero_for_one {
      // 正序遍历 tick-array
      for i in 0..self.dynamic_info.all_tick_array_state.len() {
        let tick_array = &self.dynamic_info.all_tick_array_state[i];
        if tick_array.start_tick_index >= first_tick_array_start_index {
          tick_array_dequeue.push_back(tick_array.clone());
        }
      }
    } else {
      // 反序遍历 tick-array
      for i in (0..self.dynamic_info.all_tick_array_state.len()).rev() {
        let tick_array = &self.dynamic_info.all_tick_array_state[i];
        if tick_array.start_tick_index <= first_tick_array_start_index {
          tick_array_dequeue.push_back(tick_array.clone());
        }
      }
    }
    Ok(tick_array_dequeue)
  }

  // the range of tick array start index that default tickarray bitmap can represent
  // if tick_spacing = 1, the result range is [-30720, 30720)
  pub fn tick_array_start_index_range(&self) -> (i32, i32) {
    // the range of ticks that default tickarrary can represent
    let mut max_tick_boundary = tick_array_bit_map::max_tick_in_tickarray_bitmap(self.base_info.tick_spacing);
    let mut min_tick_boundary = -max_tick_boundary;
    if max_tick_boundary > tick_math::MAX_TICK {
      max_tick_boundary = TickArrayState::get_array_start_index(tick_math::MAX_TICK, self.base_info.tick_spacing);
      // find the next tick array start index
      max_tick_boundary = max_tick_boundary + TickArrayState::tick_count(self.base_info.tick_spacing);
    }
    if min_tick_boundary < tick_math::MIN_TICK {
      min_tick_boundary = TickArrayState::get_array_start_index(tick_math::MIN_TICK, self.base_info.tick_spacing);
    }
    (min_tick_boundary, max_tick_boundary)
  }

  pub fn is_overflow_default_tickarray_bitmap(&self, tick_indexs: Vec<i32>) -> bool {
    let (min_tick_array_start_index_boundary, max_tick_array_index_boundary) = self.tick_array_start_index_range();
    for tick_index in tick_indexs {
      let tick_array_start_index = TickArrayState::get_array_start_index(tick_index, self.base_info.tick_spacing);
      if tick_array_start_index >= max_tick_array_index_boundary || tick_array_start_index < min_tick_array_start_index_boundary {
        return true;
      }
    }
    false
  }

  pub fn next_initialized_tick_array_start_index(
    &self,
    tickarray_bitmap_extension: &TickArrayBitmapExtension,
    mut last_tick_array_start_index: i32,
    zero_for_one: bool,
  ) -> Result<Option<i32>> {
    last_tick_array_start_index = TickArrayState::get_array_start_index(last_tick_array_start_index, self.base_info.tick_spacing);

    loop {
      let (is_found, start_index) = tick_array_bit_map::next_initialized_tick_array_start_index(
        U1024(self.dynamic_info.tick_array_bitmap),
        last_tick_array_start_index,
        self.base_info.tick_spacing,
        zero_for_one,
      );
      if is_found {
        return Ok(Some(start_index));
      }
      last_tick_array_start_index = start_index;

      let (is_found, start_index) = tickarray_bitmap_extension.next_initialized_tick_array_from_one_bitmap(
        last_tick_array_start_index,
        self.base_info.tick_spacing,
        zero_for_one,
      )?;
      if is_found {
        return Ok(Some(start_index));
      }
      last_tick_array_start_index = start_index;

      if last_tick_array_start_index < tick_math::MIN_TICK || last_tick_array_start_index > tick_math::MAX_TICK {
        return Ok(None);
      }
    }
  }

  pub fn get_first_initialized_tick_array(
    &self,
    tickarray_bitmap_extension: &TickArrayBitmapExtension,
    zero_for_one: bool,
  ) -> Result<(bool, i32)> {
    let (is_initialized, start_index) = if self.is_overflow_default_tickarray_bitmap(vec![self.dynamic_info.tick_current]) {
      tickarray_bitmap_extension.check_tick_array_is_initialized(
        TickArrayState::get_array_start_index(self.dynamic_info.tick_current, self.base_info.tick_spacing),
        self.base_info.tick_spacing,
      )?
    } else {
      tick_array_bit_map::check_current_tick_array_is_initialized(
        U1024(self.dynamic_info.tick_array_bitmap),
        self.dynamic_info.tick_current,
        self.base_info.tick_spacing.into(),
      )?
    };
    if is_initialized {
      return Ok((true, start_index));
    }
    let next_start_index = self.next_initialized_tick_array_start_index(
      tickarray_bitmap_extension,
      TickArrayState::get_array_start_index(self.dynamic_info.tick_current, self.base_info.tick_spacing),
      zero_for_one,
    )?;
    //todo, 返回error
    // require!(next_start_index.is_some(), ErrorCode::InsufficientLiquidityForDirection);
    return Ok((false, next_start_index.unwrap()));
  }
}
