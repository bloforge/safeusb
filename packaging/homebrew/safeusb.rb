class Safeusb < Formula
  desc "Cross-platform disk backup, restore, and USB flashing tool"
  homepage "https://github.com/bloforge/safeusb"
  url "https://github.com/bloforge/safeusb/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "REPLACE_WITH_SHA256_OF_SOURCE_TARBALL"
  license "GPL-3.0-or-later"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "safeusb", shell_output("#{bin}/safeusb --help")
  end
end
