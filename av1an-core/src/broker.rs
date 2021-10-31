use crate::{
  ffmpeg, finish_multi_progress_bar, finish_progress_bar, get_done, settings::EncodeArgs, Chunk,
  Instant, TargetQuality, Verbosity,
};
use std::{fs::File, io::Write, path::Path, sync::mpsc::Sender};

use nix::sched::{sched_setaffinity, CpuSet};
use nix::unistd::Pid;

pub struct Broker<'a> {
  pub chunk_queue: Vec<Chunk>,
  pub project: &'a EncodeArgs,
  pub target_quality: Option<TargetQuality<'a>>,
}

impl<'a> Broker<'a> {
  pub fn new(
    chunk_queue: Vec<Chunk>,
    project: &'a EncodeArgs,
    target_quality: Option<TargetQuality<'a>>,
  ) -> Self {
    Broker {
      chunk_queue,
      project,
      target_quality,
    }
  }

  #[allow(clippy::needless_pass_by_value)]
  pub fn encoding_loop(self, tx: Sender<()>) {
    if !self.chunk_queue.is_empty() {
      let (sender, receiver) = crossbeam_channel::bounded(self.chunk_queue.len());

      for chunk in &self.chunk_queue {
        sender.send(chunk.clone()).unwrap();
      }
      drop(sender);

      crossbeam_utils::thread::scope(|s| {
        let consumers: Vec<_> = (0..self.project.workers)
          .map(|i| (receiver.clone(), &self, i))
          .map(|(rx, queue, consumer_idx)| {
            let tx = tx.clone();
            s.spawn(move |_| {
              while let Ok(mut chunk) = rx.recv() {
                if queue.encode_chunk(&mut chunk, consumer_idx).is_err() {
                  tx.send(()).unwrap();
                  return Err(());
                }
              }
              Ok(())
            })
          })
          .collect();
        for consumer in consumers {
          let _ = consumer.join().unwrap();
        }
      })
      .unwrap();

      if self.project.verbosity == Verbosity::Normal {
        finish_progress_bar();
      } else if self.project.verbosity == Verbosity::Verbose {
        finish_multi_progress_bar();
      }
    }
  }

  fn encode_chunk(&self, chunk: &mut Chunk, worker_id: usize) -> Result<(), String> {
    // We assign in a round-robin fashion. Some cores may be shared if we have
    // a number of workers that is not divisible by the number of cores.
    //
    // Examples:
    // 8 workers, 8 cores
    // [1][2][3][4][5][6][7][8]
    // 8 workers, 16 cores
    // [1][1][2][2][3][3][4][4][5][5][6][6][7][7][8][8]
    // 12 workers, 16 cores
    // [1+9][1+9][2+10][2+10][3+11][3+11][4+12][4+12][5][5][6][6][7][7][8][8]
    // 16 workers, 8 cores
    // [1+9][2+10][3+11][4+12][5+13][6+14][7+15][8+16]
    let cores_per_worker = (num_cpus::get() as f32 / self.project.workers as f32).ceil() as usize;
    let mut cpu_set = CpuSet::new();
    let start = worker_id * cores_per_worker;
    let end = start + cores_per_worker;
    for i in start..end {
      cpu_set.set(i % self.project.workers).unwrap();
    }
    sched_setaffinity(Pid::from_raw(0), &cpu_set).unwrap();

    let st_time = Instant::now();

    info!("Enc: {}, {} fr", chunk.index, chunk.frames);

    if let Some(ref tq) = self.target_quality {
      tq.per_shot_target_quality_routine(chunk);
    }

    // Run all passes for this chunk
    const MAX_TRIES: usize = 3;
    for current_pass in 1..=self.project.passes {
      for r#try in 1..=MAX_TRIES {
        let res = self.project.create_pipes(chunk, current_pass, worker_id);
        if let Err((status, output)) = res {
          warn!(
            "Encoder failed (on chunk {}) with {}:\n{}",
            chunk.index,
            status,
            textwrap::indent(&output, /* 8 spaces */ "        ")
          );
          if r#try == MAX_TRIES {
            error!(
              "Encoder crashed (on chunk {}) {} times, terminating thread",
              chunk.index, MAX_TRIES
            );
            return Err(output);
          }
        } else {
          break;
        }
      }
    }

    let encoded_frames = Self::frame_check_output(chunk, chunk.frames);

    if encoded_frames == chunk.frames {
      let progress_file = Path::new(&self.project.temp).join("done.json");
      get_done().done.insert(chunk.name(), encoded_frames);

      let mut progress_file = File::create(&progress_file).unwrap();
      progress_file
        .write_all(serde_json::to_string(get_done()).unwrap().as_bytes())
        .unwrap();

      let enc_time = st_time.elapsed();

      info!(
        "Done: {} Fr: {}/{}",
        chunk.index, encoded_frames, chunk.frames
      );
      info!(
        "Fps: {:.2} Time: {:?}",
        encoded_frames as f64 / enc_time.as_secs_f64(),
        enc_time
      );
    }

    Ok(())
  }

  fn frame_check_output(chunk: &Chunk, expected_frames: usize) -> usize {
    let actual_frames = ffmpeg::num_frames(chunk.output().as_ref()).unwrap();

    if actual_frames != expected_frames {
      warn!(
        "FRAME MISMATCH: Chunk #{}: {}/{} fr",
        chunk.index, actual_frames, expected_frames
      );
    }

    actual_frames
  }
}
