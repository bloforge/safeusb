{
  description = "SafeUSB: cross-platform disk backup, restore, and USB flashing tool";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "safeusb";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.zstd pkgs.xz ];
          meta = {
            description = "Cross-platform disk backup, restore, and USB flashing tool";
            homepage = "https://github.com/bloforge/safeusb";
            license = pkgs.lib.licenses.gpl3Plus;
            mainProgram = "safeusb";
          };
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [ cargo rustc clippy rustfmt pkg-config ];
        };
      });
}
