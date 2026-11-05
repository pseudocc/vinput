{
  lib,
  stdenvNoCC,
  makeWrapper,
  python,
}:

stdenvNoCC.mkDerivation {
  pname = "vinput-asr";
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).workspace.package.version;

  dontUnpack = true;
  nativeBuildInputs = [ makeWrapper ];

  installPhase = ''
    runHook preInstall
    install -Dm644 ${../asr/server.py} "$out/libexec/vinput-asr/server.py"
    makeWrapper ${python}/bin/python3 "$out/bin/vinput-asr" \
      --add-flags "$out/libexec/vinput-asr/server.py"
    runHook postInstall
  '';

  meta = {
    description = "Local OpenVINO Whisper transcription server";
    mainProgram = "vinput-asr";
    platforms = lib.platforms.linux;
  };
}
