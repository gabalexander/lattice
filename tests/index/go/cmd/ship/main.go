package main

import "github.com/spf13/cobra"

var (
	rootCmd = &cobra.Command{Use: "ship"}
	buildCmd = &cobra.Command{
		Use:   "build [flags] DIR",
		Short: "Build a ship",
	}
	imageCmd      = &cobra.Command{Use: "image"}
	imageBuildCmd = &cobra.Command{Use: "build"}
)

func init() {
	rootCmd.AddCommand(buildCmd, imageCmd)
	imageCmd.AddCommand(imageBuildCmd)
	flags := buildCmd.Flags()
	squashFlagName := "squash"
	flags.BoolVar(&squash, squashFlagName, false, "squash the layers")
	imageBuildCmd.Flags().StringVarP(&file, "file", "f", "", "the file")
}

var squash bool
var file string

func main() {
	_ = rootCmd.Execute()
}
