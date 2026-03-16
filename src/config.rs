/*
 * szsol-rs
 * Copyright (C) 2026 ghoker143
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */
use std::fs;
use std::path::PathBuf;

use directories::ProjectDirs;

use crate::tui_renderer::AnimSpeed;

#[derive(Debug, Clone, Copy)]
pub struct AppConfig {
    pub anim_speed: AnimSpeed,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            anim_speed: AnimSpeed::Normal,
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let Some(path) = Self::file_path() else {
            return Self::default();
        };

        let Ok(content) = fs::read_to_string(path) else {
            return Self::default();
        };

        let mut config = Self::default();
        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();

            if key == "anim_speed" {
                config.anim_speed = parse_anim_speed(value).unwrap_or(AnimSpeed::Normal);
            }
        }

        config
    }

    pub fn save(&self) {
        let Some(path) = Self::file_path() else {
            return;
        };

        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }

        let content = format!(
            "# szsol-rs config\nanim_speed = {}\n",
            anim_speed_name(self.anim_speed)
        );

        let _ = fs::write(path, content);
    }

    fn file_path() -> Option<PathBuf> {
        let proj_dirs = ProjectDirs::from("com", "szsol", "szsol")?;
        Some(proj_dirs.config_dir().join("config.txt"))
    }
}

fn parse_anim_speed(value: &str) -> Option<AnimSpeed> {
    match value.trim().to_ascii_lowercase().as_str() {
        "off" => Some(AnimSpeed::Off),
        "fast" => Some(AnimSpeed::Fast),
        "normal" => Some(AnimSpeed::Normal),
        "slow" => Some(AnimSpeed::Slow),
        _ => None,
    }
}

fn anim_speed_name(speed: AnimSpeed) -> &'static str {
    match speed {
        AnimSpeed::Off => "off",
        AnimSpeed::Fast => "fast",
        AnimSpeed::Normal => "normal",
        AnimSpeed::Slow => "slow",
    }
}
