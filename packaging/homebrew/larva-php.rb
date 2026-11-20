class LarvaPhp < Formula
  desc "A modern PHP application server written in Rust"
  homepage "https://github.com/larva-php/larva-php"
  url "https://github.com/larva-php/larva-php/archive/refs/tags/v1.13.5.tar.gz"
  version "1.13.5"
  sha256 :no_check
  license "MIT"

  depends_on "rust" => :build
  depends_on "php"

  def install
    system "cargo", "install", *std_cargo_args
  end

  def caveats
    <<~EOS
      Serve a PHP application with:
        larva-php --root /path/to/app --addr 127.0.0.1:8000
    EOS
  end

  test do
    assert_match "LarvaPHP", shell_output("#{bin}/larva-php --version")
  end
end