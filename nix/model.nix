{
  lib,
  fetchurl,
  runCommandLocal,
}:

let
  revision = "b568445dd5dc8c695bde596f8acbb4694fd6ba64";
  hashes = {
    "added_tokens.json" = "sha256-PFH2bEwh+eEmlwB48RrnenjHSu6N9gbunauobkZxCOA=";
    "config.json" = "sha256-v9ksCXVHqxLLQquugAi+WlmpH9xas5rM4kSJ64o+ioY=";
    "generation_config.json" = "sha256-Rhf8ykWK87kaEDFDqqyRnBq2aAtVLXq9EIEbcki9d7Q=";
    "merges.txt" = "sha256-LfKZCjleNejfvHUR4IwS1WAY2NBGkeATPl1jsh4VTcY=";
    "normalizer.json" = "sha256-vxxQfchyTKnPmQNkDaz7adri8A7e5PIc66EGpzkvJt0=";
    "openvino_config.json" = "sha256-naOOiM7Aaa1UaZ1ifxxZ02w2o9+1S14btc/fWDLvrwM=";
    "openvino_decoder_model.bin" = "sha256-wGSZHLr8Q4FWfSmXK3AT3CQCbenDJtA+sebk/ESqlZ8=";
    "openvino_decoder_model.xml" = "sha256-rrCfr78cDL+EuvMPRnY0NgBYIvrzs2U1m43gqgTwMEc=";
    "openvino_detokenizer.bin" = "sha256-8rPEeCWhCJUl/2XAyOSSceHe5ppAGgT8gnrC3lt3ZuQ=";
    "openvino_detokenizer.xml" = "sha256-bhBqFPFLB3G0a3lIqZsdgZ/5OyRVt9qPR3Yaudup3FY=";
    "openvino_encoder_model.bin" = "sha256-BZCo81+W1XgBxVmQAo2ReCGschAm40t/P1nXVh/JCOY=";
    "openvino_encoder_model.xml" = "sha256-YHE9TtOorI7gIOEcRzfsJ20UyrxqCCU3vd8sALps4HA=";
    "openvino_tokenizer.bin" = "sha256-rfo9mikg0PMUEhJwqWCrMx7A8Fg4VEu47KqkIigqb9Q=";
    "openvino_tokenizer.xml" = "sha256-y6ME57rVR3O50svM+8hQERfs8uPA9PUzF0Kgo8n+7ZM=";
    "preprocessor_config.json" = "sha256-ZUzxjT4WO5SM6vl2baVs4LUt4mXVhnPPYck3bxJr1Jk=";
    "special_tokens_map.json" = "sha256-uupOoJNy60/KhrTkNGE5/XPLgH1Qh+neCUjpcXOcPnQ=";
    "tokenizer_config.json" = "sha256-PHWUDfzjopT8pwQaX6/wEWd/G2j6heR1EbuM9tzK3tY=";
    "tokenizer.json" = "sha256-XBvzDJ5xbhR3vt74RrAb4AE9rsuJ6ePveribI8F43xs=";
    "vocab.json" = "sha256-Z4jICwgumw0TkxR9Oj5iuhkoWsDIKs6OXvAPN+rViXE=";
  };
in
runCommandLocal "whisper-large-v3-turbo-int8-ov-${builtins.substring 0 7 revision}"
  {
    meta = {
      description = "OpenVINO Whisper large-v3-turbo INT8 speech model";
      homepage = "https://huggingface.co/OpenVINO/whisper-large-v3-turbo-int8-ov";
      license = lib.licenses.mit;
      platforms = lib.platforms.linux;
    };
  }
  ''
    mkdir -p "$out"
    ${lib.concatStringsSep "\n" (
      lib.mapAttrsToList (name: hash: ''
        ln -s ${
          fetchurl {
            url = "https://huggingface.co/OpenVINO/whisper-large-v3-turbo-int8-ov/resolve/${revision}/${name}";
            inherit hash;
          }
        } "$out/${name}"
      '') hashes
    )}
  ''
