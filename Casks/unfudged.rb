cask "unfudged" do
  version "0.18.1"
  sha256 "92c63c84f3216a2a74903615deac511d9aeb1a0a4e92bba3c1ff29e9e14eeea5"

  url "https://downloads.unfudged.io/releases/v0.18.1/UNFUDGED-v0.18.1-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
