// #[cfg(feature = "mkl")]
// extern crate intel_mkl_src;

// #[cfg(feature = "accelerate")]
// extern crate accelerate_src;
// TODO: Zen

use anyhow::Result;
use clap::Parser;

use crate::ml::end::LMEndShard;
use crate::ml::link::LMLinkShard;
use crate::ml::model::Model;
use crate::ml::origin::LMOriginShard;
use crate::ml::origin::OriginInput;
use crate::ml::util::Args;

fn input(next_token_idx: Option<u32>, prompt: String) -> MLInput {
    if let Some(next_token_idx) = next_token_idx {
        MLInput::NextTokIdx(next_token_idx)
    } else {
        MLInput::Prompt(prompt)
    }
}

fn integrity_test() -> Result<()> {
    let args = Args::parse();

    let mut shard_0 = LMOriginShard::new(&args)?;
    let mut shard_1 = LMLinkShard::new(&args, 1)?;
    let mut shard_2 = LMLinkShard::new(&args, 2)?;
    let mut shard_3 = LMEndShard::new(&args, 3)?;

    let mut next_token_idx: Option<u32> = None;

    for iteration in 0..50 {
        let input = input(next_token_idx, args.prompt.clone());
        println!("Iteration {}", iteration);

        println!("Shard 0");
        // TODO: Helper function
        let (activation, start_pos) = shard_0.forward(input)?;
        println!("Shape of the activation is {:?}", activation.shape());
        shard_0.unload_model();

        println!("Shard 1");
        let activation = shard_1.forward(&activation)?;
        println!("Shape of the activation is {:?}", activation.shape());
        shard_1.unload_model();

        println!("Shard 2");
        let activation = shard_2.forward(&activation)?;
        println!("Shape of the activation is {:?}", activation.shape());
        shard_2.unload_model();

        println!("Shard 3");
        next_token_idx = Some(shard_3.forward(&activation)?);
        shard_3.unload_model();
    }
    Ok(())
}

// fn speed_test() -> Result<()> {
//     let args = Args::parse();

//     let mut shard_1 = LinkProcessor::new(&args, 1)?;
//     let input = Tensor::zeros(&[1, 1, 4096], DType::F16, &shard_1.device)?;

//     for iteration in 0..500 {
//         let start = std::time::Instant::now();
//         let activation = shard_1.forward(&input, iteration)?;
//         if iteration % 100 == 0 {
//             std::thread::sleep(std::time::Duration::from_secs(5));
//         }
//         println!("Iteration {} took {:?}", iteration, start.elapsed());
//     }
//     Ok(())
// }

fn main() -> Result<()> {
    integrity_test()
    // speed_test()
}

/*
TODO: Zen:

Troubleshooting for useless output:
- Are the correct shards being loaded?
    - seems like it
- Is the temperature correct?



Tests:
    - Test if the answer is coherent, give a good test, and compare with online versions of mixtral.
    - Also time each shard to make sure there are no disparities

Can the forward method use branching?
I think origin/link/end could be a single model, with a branching forward method?
Maybe we can merge processor and model? I wouldn't do it for now, but it's a possibility

 */

/*
TODO: --features metal
cargo run --release --features metal -- \
--prompt 'What man is to woman, king is to queen. Give 5 more examples of this.' \
--sample-len 150 \
--weight-folder "../candle-original/weights/sharded_mixtral/" \
--tokenizer-file "../candle-original/weights/sharded_mixtral/tokenizer.json"
*/
