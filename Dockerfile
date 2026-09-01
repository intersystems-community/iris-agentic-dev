# TARGETARCH is set by docker buildx: "amd64" or "arm64"
# Re-declare after FROM so the variable is available in COPY.
ARG TARGETARCH=amd64
FROM gcr.io/distroless/static-debian12
ARG TARGETARCH=amd64
COPY bin/iris-agentic-dev-${TARGETARCH} /iris-agentic-dev
ENTRYPOINT ["/iris-agentic-dev"]
