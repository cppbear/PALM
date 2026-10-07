FROM ubuntu:24.04

ARG USERNAME=palm
ARG DEBIAN_FRONTEND=noninteractive
ARG TARGETARCH

# Set mirror for Ubuntu
# RUN if echo "$TARGETARCH" | grep -q "arm"; then \
#     sed -i.bak 's|http://ports.ubuntu.com/ubuntu-ports|<xxxxxxxx>|g' /etc/apt/sources.list.d/ubuntu.sources; \
#     else \
#     sed -i.bak 's|http://archive.ubuntu.com/ubuntu|<xxxxxxxx>|g' /etc/apt/sources.list.d/ubuntu.sources; \
#     fi

RUN apt update && apt upgrade -y \
    && apt install -y ca-certificates \
    # Install packages
    && apt install -y build-essential git curl wget vim \
    sudo \
    zsh zsh-autosuggestions zsh-syntax-highlighting \
    && rm -rf /var/lib/apt/lists/* \
    # Set up user
    && userdel -r ubuntu \
    && useradd -m -s /usr/bin/zsh -G sudo "$USERNAME" \
    && echo "$USERNAME ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers.d/$USERNAME && \
    chmod 0440 /etc/sudoers.d/$USERNAME

USER $USERNAME
WORKDIR /home/$USERNAME

RUN sh -c "$(curl -fsSL https://raw.githubusercontent.com/ohmyzsh/ohmyzsh/master/tools/install.sh)" \
    && sed -i 's/^ZSH_THEME=.*/ZSH_THEME="ys"/' ~/.zshrc \
    && echo 'source /usr/share/zsh-autosuggestions/zsh-autosuggestions.zsh' >> ~/.zshrc \
    && echo 'source /usr/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh' >> ~/.zshrc \
    # Install rustup
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain=none \
    && . "$HOME/.cargo/env" \
    # Set mirror for crates.io
    # ...
    && rustup install nightly-2025-03-19 \
    && rustup component add --toolchain nightly-2025-03-19 rust-src rustc-dev llvm-tools-preview \
    && host=$(rustc -vV | grep '^host:' | cut -d' ' -f2); \
    curl --proto '=https' --tlsv1.2 -fsSL \
    "https://github.com/taiki-e/cargo-llvm-cov/releases/download/v0.6.16/cargo-llvm-cov-$host.tar.gz" \
    | tar xzf - -C "$HOME/.cargo/bin"

CMD ["/usr/bin/zsh"]
