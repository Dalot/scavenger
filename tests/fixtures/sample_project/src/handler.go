package main

import "fmt"

type Handler struct {
	Name    string
	Timeout int
}

type Request struct {
	Method string
	Path   string
	Body   []byte
}

type Response struct {
	Status int
	Body   string
}

func NewHandler(name string) *Handler {
	return &Handler{Name: name, Timeout: 30}
}

func (h *Handler) Handle(req *Request) *Response {
	fmt.Printf("Handling %s %s\n", req.Method, req.Path)
	return &Response{Status: 200, Body: "OK"}
}

func ProcessRequest(req *Request) *Response {
	handler := NewHandler("default")
	return handler.Handle(req)
}

type Router interface {
	Route(path string) *Handler
	Register(path string, handler *Handler)
}
