cask "unfudged" do
  version "0.17.8"
  sha256 "887714e6316e4aec609eed56ace32ffc290ceb76899900abd78f5debc81f9f5c"

  url "https://downloads.unfudged.io/releases/v0.17.8/UNFUDGED-v0.17.8-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
