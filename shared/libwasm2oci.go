package main

import (
	"C"

	log "github.com/sirupsen/logrus"

	"github.com/engineerd/wasm-to-oci/pkg/oci"
)

//export Pull
func Pull(ref, outFile string) int64 {
	err := oci.Pull(ref, outFile)
	if err != nil {
		log.Infof("cannot pull module: %v", err)
		return 1
	}

	return 0
}

//export Push
func Push(ref, mod string) int64 {
	err := oci.Push(ref, mod)
	if err != nil {
		log.Infof("cannot push module: %v", err)
		return 1
	}

	return 0
}

func main() {}
