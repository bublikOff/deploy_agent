# Use the official Rust image for building
FROM rust:latest AS builder

# Install required runtime dependencies
RUN apt-get update && apt-get -y install wget curl

# Set the working directory
WORKDIR /build

# Copy the source code to the container
COPY ./sources .

# Build the release version of the Rust app
RUN cargo build --release

# Use an export stage to limit files
FROM scratch AS export-stage

# Work in a clean directory
WORKDIR /

# Copy only the necessary files to /output
COPY --from=builder /build/target/release/deploy_agent .
