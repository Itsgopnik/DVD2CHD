use super::App;
use dvd2chd_core::core_wiring::{new_channel, spawn_core_job, Mode};
use dvd2chd_core::{ArchiveOptions, ExtractOptions, FileOptions};
use rust_i18n::t;
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc};

use super::workflow::TimelineKind;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum JobIntent {
    File,
    Device,
}

impl App {
    pub(super) fn current_job_intent(&self) -> Option<JobIntent> {
        if self.s.source_file.is_some() {
            Some(JobIntent::File)
        } else if self.s.device_path.is_some() {
            Some(JobIntent::Device)
        } else {
            None
        }
    }

    pub(super) fn should_prompt_for_custom_name(&self) -> bool {
        let Some(dev) = self.s.device_path.as_ref() else {
            return false;
        };
        if !matches!(self.current_job_intent(), Some(JobIntent::Device)) {
            return false;
        }
        if !self.s.prefer_id_rename && !self.s.rename_by_label {
            return true;
        }
        if self.s.prefer_id_rename && self.ps_id_from_source(dev).is_some() {
            return false;
        }
        if self.s.rename_by_label && self.read_volume_label_from_source(dev).is_some() {
            return false;
        }
        true
    }

    pub(super) fn start_job(&mut self) {
        if self.show_custom_name_prompt {
            return;
        }
        if self.should_prompt_for_custom_name() {
            self.custom_name_error = None;
            self.custom_name_input.clear();
            self.show_custom_name_prompt = true;
            return;
        }
        self.start_job_internal(None);
    }

    pub(super) fn start_job_internal(&mut self, custom_name: Option<String>) {
        self.reprobe_tools();
        let out_dir = if let Some(d) = &self.s.out_dir {
            d.clone()
        } else {
            self.log_line(&t!("log.no_output_folder"));
            return;
        };

        let intent = if let Some(i) = self.current_job_intent() {
            i
        } else {
            self.log_line(&t!("log.no_source"));
            return;
        };

        let missing = self.missing_tools_for(intent);
        if !missing.is_empty() {
            for req in missing {
                self.log_line(&format!("✖ {}\n", t!(req.reason_key())));
            }
            return;
        }

        self.log_open = true;
        self.timeline.clear();
        self.job_start_time = Some(std::time::Instant::now());
        let (tx, rx) = new_channel();
        self.rx = Some(rx);
        self.is_extract_job = false;
        self.progress_hide_at = None;
        self.running = true;
        self.progress = 0.0;
        self.label.clear();
        if self.s.auto_clear_log {
            self.clear_log();
        } else if !self.log.is_empty() {
            if !self.log_ends_with_newline() {
                self.append_log_text("\n");
            }
            self.append_log_text("---\n");
        }

        #[cfg(debug_assertions)]
        {
            self.debug_animation_override = None;
            self.debug_update_indicator(None);
            self.debug_timeline_backup = None;
        }

        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());

        let wants_hashes = self.s.compute_md5 || self.s.compute_sha1 || self.s.compute_sha256;

        if let Some(f) = self.s.source_file.clone() {
            if !f.exists() {
                self.running = false;
                self.cancel = None;
                self.log_line(&t!("log.source_not_found", path = f.display().to_string()));
                return;
            }
            self.prepare_timeline(TimelineKind::File, wants_hashes);
            let fo = FileOptions {
                force_createdvd: self.s.force_createdvd,
                extra_chd_args: self.s.extra_chd_args.clone(),
                run_nice: self.s.run_nice,
                run_ionice: self.s.run_ionice,
                compute_md5: self.s.compute_md5,
                compute_sha1: self.s.compute_sha1,
                compute_sha256: self.s.compute_sha256,
                delete_image_after: self.s.delete_image_after,
                chdman_path: self.s.chdman_path.clone(),
            };
            let mode = Mode::File {
                in_path: f,
                out_dir,
                opts: fo,
            };
            let _ = spawn_core_job(mode, tx, cancel);
            return;
        }

        if let Some(dev) = self.s.device_path.clone() {
            self.prepare_timeline(TimelineKind::Device, wants_hashes);
            let ao = ArchiveOptions {
                out_dir: Some(out_dir.clone()),
                custom_name,
                use_ddrescue: self.s.use_ddrescue,
                ddrescue_scrape: self.s.ddrescue_scrape,
                prefer_id_rename: self.s.prefer_id_rename,
                rename_by_label: self.s.rename_by_label,
                delete_image_after: self.s.delete_image_after,
                cd_speed_x: self.s.cd_speed_x,
                cd_buffers: self.s.cd_buffers,
                extra_chd_args: self.s.extra_chd_args.clone(),
                run_nice: self.s.run_nice,
                run_ionice: self.s.run_ionice,
                compute_md5: self.s.compute_md5,
                compute_sha1: self.s.compute_sha1,
                compute_sha256: self.s.compute_sha256,
                auto_eject: self.s.auto_eject,
                chdman_path: self.s.chdman_path.clone(),
                ddrescue_path: self.s.ddrescue_path.clone(),
                cdrdao_path: self.s.cdrdao_path.clone(),
            };
            let mode = Mode::Device {
                dev_path: dev,
                profile: self.s.profile,
                opts: ao,
            };
            let _ = spawn_core_job(mode, tx, cancel);
        }
    }

    pub(super) fn start_extract_job(&mut self) {
        let Some(in_path) = self.extract_chd_path.clone() else {
            self.log_line(&t!("log.extract_no_chd"));
            return;
        };
        let Some(out_dir) = self.extract_out_dir.clone() else {
            self.log_line(&t!("log.no_output_folder"));
            return;
        };
        if self.running {
            return;
        }
        if self.tools.chdman.is_none() {
            self.log_line(&t!("tool.chdman_missing"));
            return;
        }

        self.log_open = true;
        self.timeline.clear();
        self.job_start_time = Some(std::time::Instant::now());
        let (tx, rx) = new_channel();
        self.rx = Some(rx);
        self.progress_hide_at = None;
        self.running = true;
        self.is_extract_job = true;
        self.progress = 0.0;
        self.label.clear();
        if self.s.auto_clear_log {
            self.clear_log();
        } else if !self.log.is_empty() {
            if !self.log_ends_with_newline() {
                self.append_log_text("\n");
            }
            self.append_log_text("---\n");
        }

        self.log_line(&t!(
            "log.extract_start",
            path = in_path.display().to_string()
        ));

        #[cfg(debug_assertions)]
        {
            self.debug_animation_override = None;
            self.debug_update_indicator(None);
            self.debug_timeline_backup = None;
        }

        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());

        let opts = ExtractOptions {
            mode: self.extract_mode,
            run_nice: self.s.run_nice,
            run_ionice: self.s.run_ionice,
            chdman_path: self.s.chdman_path.clone(),
        };
        let mode = Mode::Extract {
            in_path,
            out_dir,
            opts,
        };
        let _ = spawn_core_job(mode, tx, cancel);
    }

    pub(super) fn enqueue_batch_file(&mut self, path: PathBuf) {
        if self.batch_queue.iter().any(|p| p == &path) {
            self.log_line(&t!(
                "log.batch_duplicate",
                path = path.display().to_string()
            ));
            return;
        }
        self.log_line(&t!("log.batch_added", path = path.display().to_string()));
        self.batch_queue.push_back(path);
    }

    pub(super) fn start_next_batch_if_possible(&mut self) -> bool {
        if self.running {
            return false;
        }
        let Some(next) = self.batch_queue.pop_front() else {
            return false;
        };
        if self.s.out_dir.is_none() {
            if let Some(parent) = next.parent() {
                self.s.out_dir = Some(parent.to_path_buf());
            } else {
                self.log_line(&t!("log.batch_no_target"));
                self.batch_queue.push_front(next);
                return false;
            }
        }
        self.set_source_file(next.clone(), None);
        self.log_line(&t!("log.batch_start", path = next.display().to_string()));
        self.start_job();
        true
    }
}
