package main

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/BurntSushi/toml"
)

type Config struct {
	WindowClass   string `toml:"window_class"`
	StartCommand  string `toml:"start_command"`
	Hotkey        string `toml:"hotkey"`
	DisplayMode   string `toml:"display_mode"`
	DisplayIndex  int    `toml:"display_index"`
	WidthPercent  int    `toml:"width_percent"`
	HeightPercent int    `toml:"height_percent"`

	ShowDuration int    `toml:"show_duration"`
	HideDuration int    `toml:"hide_duration"`
	ShowEasing   string `toml:"show_easing"`
	HideEasing   string `toml:"hide_easing"`

	// Terminal logic
	WidthCols  int `toml:"width_cols"`
	HeightRows int `toml:"height_rows"`
}

func findConfigFile() string {
	configFiles := []string{".goake.toml"}
	home, err := os.UserHomeDir()
	if err == nil {
		configFiles = append(configFiles, filepath.Join(home, ".goake.toml"))
		xdgConfig := os.Getenv("XDG_CONFIG_HOME")
		if xdgConfig == "" {
			xdgConfig = filepath.Join(home, ".config")
		}
		configFiles = append(configFiles, filepath.Join(xdgConfig, "goake", ".goake.toml"))
	}
	for _, path := range configFiles {
		if _, err := os.Stat(path); err == nil {
			return path
		}
	}
	return ""
}

func loadConfig() Config {
	path := findConfigFile()

	var config Config
	if path != "" {
		if _, err := toml.DecodeFile(path, &config); err == nil {
			fmt.Printf("Loaded config from: %s\n", path)
		} else {
			fmt.Printf("Error decoding config: %v\n", err)
		}
	} else {
		fmt.Println("No config file found. Using defaults.")
		config = Config{
			WindowClass:   "wezquake",
			StartCommand:  "wezterm-gui start",
			Hotkey:        "Meta+Grave",
			DisplayMode:   "follow-mouse",
			WidthPercent:  40,
			HeightPercent: 40,
			ShowDuration:  300,
			HideDuration:  300,
			ShowEasing:    "ease-out",
			HideEasing:    "ease-in",
			WidthCols:     120,
			HeightRows:    40,
		}
	}
	// Default easings if missing
	if config.ShowEasing == "" {
		config.ShowEasing = "ease-out"
	}
	if config.HideEasing == "" {
		config.HideEasing = "ease-in"
	}
	return config
}
