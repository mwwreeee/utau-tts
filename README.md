
# utau-tts

  

Text-to-speech using UTAU voicebanks only english is currently supported

no compiler, please run  `cargo test` first.

English is approximated through Japanese phonology

## Commands
`cargo run -p utau-tts -- inspect --bank "/path/"`

`cargo run -p utau-tts -- say --bank "/path/" --dict cmudict.dict --text "Hello, world." `

  
