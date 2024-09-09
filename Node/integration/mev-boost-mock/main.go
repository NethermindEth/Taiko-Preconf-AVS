// main.go
package main

import (
	"encoding/json"
	"fmt"
	"log"
	"net/http"

	"github.com/gorilla/mux"
	"github.com/flashbots/go-boost-utils/utils"
	"github.com/flashbots/go-boost-utils/bls"
	//"github.com/flashbots/go-boost-utils/ssz"
)

// Define the structure of the incoming data
type ConstraintData struct {
	Data string `json:"data"`
}

const pathSubmitConstraint = "/eth/v1/builder/constraints"
// Handler function for the POST request
func ConstraintsHandler(w http.ResponseWriter, r *http.Request) {
	// Parse the incoming JSON body
	var requestData ConstraintData
	err := json.NewDecoder(r.Body).Decode(&requestData)
	if err != nil {
		http.Error(w, "Invalid request payload", http.StatusBadRequest)
		return
	}

	// Print the received data (for demonstration purposes)
	fmt.Printf("Received data: %s\n", requestData.Data)

	// Respond to the client
	response := map[string]string{
		"message": "Constraints data received successfully",
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(response)
}

func main() {
	// Create a new router
	router := mux.NewRouter()

	// Define the route and the handler
	router.HandleFunc(pathSubmitConstraint, handleSubmitConstraint).Methods(http.MethodPost)

	// Start the server
	fmt.Println("Server running on port 8080")
	log.Fatal(http.ListenAndServe(":8080", router))
}

func handleSubmitConstraint(w http.ResponseWriter, req *http.Request) {

	fmt.Println("submitConstraint")

	payload := BatchedSignedConstraints{}
	if err := DecodeJSON(req.Body, &payload); err != nil {
		fmt.Println("error decoding payload: ", err)
		//m.respondError(w, http.StatusBadRequest, err.Error())
		return
	}

	result := "VerifySignature: true";

	for _, signedConstraints := range payload {
		fmt.Println("SignedConstraint:", signedConstraints)
		// NOTE: publicKey is hardcoded here
		proposerPubKeyStr := "0xa45723f1721da6459705bcce04c84c54738e60d58c37b554b549bc4a297d5867e5c0d196d85dcb0e2a26c798d2908051";
		proposerPubKey, err := utils.HexToPubkey(proposerPubKeyStr)
		if err != nil {
			fmt.Println("could not convert pubkey to phase0.BLSPubKey: ", err)
			return
		}
		blsPublicKey, err := bls.PublicKeyFromBytes(proposerPubKey[:])
		if err != nil {
			fmt.Println("could not convert proposer pubkey to bls.PublicKey: ", err)
			return
		}

		// Verify signature
		signature, err := bls.SignatureFromBytes(signedConstraints.Signature[:])
		if err != nil {
			fmt.Println("could not convert signature to bls.Signature: ", err)
			return
		}

		message := signedConstraints.Message

		// NOTE: even if payload is sent with JSON, the signature digest is the SSZ encoding of the message
		messageSSZ, err := message.MarshalSSZ()
		fmt.Println("messageSSZ: ", messageSSZ)
		if err != nil {
			fmt.Println("could not marshal constraint message to json: ", err)
			return
		}
		sigRes, err := bls.VerifySignature(signature, blsPublicKey, messageSSZ)
		if err != nil {
			fmt.Println("error while veryfing signature: ", err)
			return
		}
		fmt.Println("VerifySignature: ", sigRes)
		if sigRes != true {
			result = "VerifySignature: false";
		}
	}

	// Respond to the client
	response := map[string]string{
		"message": result,
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(response)
}
