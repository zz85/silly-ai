# Homebrew formula for Silly AI
#
# Install directly from this repo:
#   brew install zz85/silly-ai/silly
#
# Or via a dedicated tap (if zz85/homebrew-tap exists):
#   brew tap zz85/tap
#   brew install silly
#
# After a new release, update VERSION and sha256 values below.
# To compute sha256: shasum -a 256 silly-darwin-aarch64.tar.gz

class Silly < Formula
  desc "Local AI voice assistant with speech-to-text, LLM, and text-to-speech"
  homepage "https://github.com/zz85/silly-ai"
  version "0.3.1"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/zz85/silly-ai/releases/download/v#{version}/silly-darwin-aarch64.tar.gz"
      sha256 "PLACEHOLDER_SHA256_DARWIN_AARCH64"
    elsif Hardware::CPU.intel?
      url "https://github.com/zz85/silly-ai/releases/download/v#{version}/silly-darwin-x86_64.tar.gz"
      sha256 "PLACEHOLDER_SHA256_DARWIN_X86_64"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/zz85/silly-ai/releases/download/v#{version}/silly-linux-x86_64.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX_X86_64"
    end
  end

  def install
    bin.install "silly"
  end

  def caveats
    <<~EOS
      On first run, silly will download ~500MB of AI models to:
        ~/.local/share/silly/models/

      This includes speech-to-text, voice activity detection, and
      text-to-speech models. Ensure you have an internet connection
      for the initial setup.

      Get started:
        silly --help
    EOS
  end

  test do
    assert_match "silly", shell_output("#{bin}/silly --help", 0)
  end
end
