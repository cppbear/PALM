#!/usr/bin/bash

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
NC='\033[0m' # Reset color

default_projects=("brinfo" "focxt/call_chain" "focxt" "utgen")

install_project() {
    local project=$1
    local original_path=$(pwd)
    printf "Installing ${BLUE}$project${NC}...\n"
    cd $project
    cargo install --path . --locked &> /dev/null
    if [ $? -eq 0 ]; then
        printf "${GREEN}$project installed successfully.${NC}\n"
    else
        printf "${RED}Failed to install $project.${NC}\n"
    fi
    cd $original_path
}

cargo clean &> /dev/null

if [ $# -eq 0 ]; then
    printf "No projects specified. Installing default projects: ${YELLOW}${default_projects[*]}${NC}\n"
    for project in "${default_projects[@]}"; do
        install_project "$project"
    done
else
    printf "Installing specified projects: ${YELLOW}$*${NC}\n"
    for project in "$@"; do
        install_project "$project"
    done
fi
