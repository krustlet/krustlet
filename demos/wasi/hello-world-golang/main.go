package main

import (
	"fmt"
	"os"
)

func main() {
	fmt.Fprintf(os.Stdout, "hello from stdout!\n")
	fmt.Fprintf(os.Stderr, "hello from stderr!\n")
	for _, e := range os.Environ() {
		fmt.Printf("%s\n", e)
	}
	fmt.Printf("Args are: %s", os.Args)
}
