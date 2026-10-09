# lattice's Homebrew formula, for a tap of its own: it installs a release's
# archive for the machine, checked against its checksum. formula.sh, beside
# this file in lattice's repository, fills in a release's version, where
# its archives are and their checksums:
#
#   packaging/homebrew/formula.sh 0.1.0 > Formula/lattice.rb
#
# brew install gabalexander/lattice/lattice
#
# Adapted from crystal's packaging/homebrew/crystal.rb (MIT).
class Lattice < Formula
  desc "Code wiki for your own repositories, written by Claude Code"
  homepage "https://github.com/gabalexander/lattice"
  version "@VERSION@"
  license "MIT"

  on_macos do
    on_arm do
      url "@RELEASES@/download/v@VERSION@/lattice-@VERSION@-aarch64-apple-darwin.tar.gz"
      sha256 "@SHA256_aarch64-apple-darwin@"
    end
    on_intel do
      url "@RELEASES@/download/v@VERSION@/lattice-@VERSION@-x86_64-apple-darwin.tar.gz"
      sha256 "@SHA256_x86_64-apple-darwin@"
    end
  end

  on_linux do
    on_arm do
      url "@RELEASES@/download/v@VERSION@/lattice-@VERSION@-aarch64-unknown-linux-musl.tar.gz"
      sha256 "@SHA256_aarch64-unknown-linux-musl@"
    end
    on_intel do
      url "@RELEASES@/download/v@VERSION@/lattice-@VERSION@-x86_64-unknown-linux-musl.tar.gz"
      sha256 "@SHA256_x86_64-unknown-linux-musl@"
    end
  end

  def install
    bin.install "lattice"
  end

  def caveats
    <<~EOS
      lattice runs Claude Code for everything it asks of a model: install it
      and log in, then check the setup with:
        lattice doctor
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/lattice --version")
  end
end
