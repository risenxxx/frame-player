# A static binary on `scratch`: no shell, no package manager, no libc, nothing
# to keep patched. The relay opens one listening socket and touches no files, so
# there is nothing else an image needs to carry.
#
# The build stage runs on the builder's own architecture and cross-compiles to
# the target's: a pure-Go binary needs nothing but GOOS/GOARCH for that, and it
# keeps the arm64 half of the published image out of QEMU, which is several
# times slower and is the only thing emulation would have been for.
FROM --platform=$BUILDPLATFORM golang:1.24-alpine AS build
ARG TARGETOS TARGETARCH
WORKDIR /src
# Copied first so a dependency layer survives a code change.
COPY go.mod go.sum ./
RUN go mod download
COPY . .
# CGO off is what makes the binary static; trimpath and -w -s keep it small and
# free of local build paths.
RUN CGO_ENABLED=0 GOOS=$TARGETOS GOARCH=$TARGETARCH go build -trimpath -ldflags "-s -w" -o /relay .

FROM scratch
COPY --from=build /relay /relay
# No state is kept, so an unprivileged uid needs nothing to own.
USER 65534:65534
EXPOSE 8080
ENTRYPOINT ["/relay"]
