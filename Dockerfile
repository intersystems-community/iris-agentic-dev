# TARGETARCH is set by docker buildx: "amd64" or "arm64"
# Re-declare after FROM so the variable is available in COPY.
ARG TARGETARCH=amd64
# The :nonroot variant, pinned by the digest of its multi-arch index so buildx still picks the
# amd64 or arm64 manifest. A bare tag is whatever distroless pushed last; the digest changes only
# when this line does. To move it: docker buildx imagetools inspect gcr.io/distroless/static-debian12:nonroot
FROM gcr.io/distroless/static-debian12:nonroot@sha256:afa5c872c891853ca7fcf1f12c3edb23f7eeef36189728842dd51042ff57f7ab
ARG TARGETARCH=amd64
COPY bin/iris-agentic-dev-${TARGETARCH} /iris-agentic-dev
# The server only talks to IRIS over HTTP and reads its config from the environment, so it has no
# need for root. The image already defines nonroot (65532); saying so here keeps it explicit.
USER nonroot:nonroot
ENTRYPOINT ["/iris-agentic-dev"]
