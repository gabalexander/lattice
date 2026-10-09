// Package engine serves ships.
package engine

// HomeEnv names the directory ships are kept in.
const HomeEnv = "SHIP_HOME"

// Config is what ship.toml holds.
type Config struct {
	Name    string `toml:"name"`
	Workers int    `toml:"worker_count" json:"workers"`
}

// Server serves ships.
type Server struct {
	config Config
}

// Handler handles a ship.
type Handler interface {
	Handle(name string) error
}

// NewServer makes a server.
func NewServer(config Config) *Server {
	return &Server{config: config}
}

// Serve serves until it's stopped.
func (s *Server) Serve() error {
	return nil
}

// Stop stops it.
func (s *Server) Stop() {}
