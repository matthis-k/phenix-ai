---@meta

---@class phenix.Error
---@field kind string
---@field code string
---@field message string
---@field details? any

---@class phenix.ClientStatus
---@field state "connecting"|"ready"|"failed"|"closed"
---@field error? phenix.Error

---@class phenix.SessionStatus
---@field session_id string
---@field title? string
---@field working_directory? string
---@field execution_id? string
---@field execution_state? "pending"|"running"|"completed"|"cancelled"|"failed"
---@field settled boolean
---@field model_id? string
---@field model_name? string
---@field model_effective? boolean
---@field routing_profile_id? string
---@field routing_profile_name? string
---@field routing_effective? boolean

---@class phenix.SessionInfo
---@field session_id string
---@field title? string
---@field working_directory string

---@class phenix.TextContent
---@field kind "text"
---@field text string

---@class phenix.ResourceContent
---@field kind "resource"
---@field uri string
---@field mime_type? string
---@field text? string

---@class phenix.ImageContent
---@field kind "image"
---@field mime_type string
---@field data string

---@alias phenix.Content phenix.TextContent|phenix.ResourceContent|phenix.ImageContent

---@class phenix.Request
---@field poll fun(self: phenix.Request): ready:boolean, value?:any, error?:phenix.Error

---@class phenix.Sessions
---@field create fun(self: phenix.Sessions, options:{working_directory:string,title?:string}):phenix.Request
---@field list fun(self: phenix.Sessions, options?:{cursor?:string}):phenix.Request
---@field resume fun(self: phenix.Sessions, session_id:string):phenix.Request
---@field cached fun(self: phenix.Sessions, session_id:string):phenix.Session?

---@class phenix.Session
---@field id fun(self: phenix.Session):string
---@field info fun(self: phenix.Session):phenix.SessionInfo?
---@field projection fun(self: phenix.Session):table?
---@field status fun(self: phenix.Session):phenix.SessionStatus
---@field prompt fun(self: phenix.Session, content:phenix.Content[]):phenix.Request
---@field cancel fun(self: phenix.Session):phenix.Request
---@field close fun(self: phenix.Session):phenix.Request
---@field rename fun(self: phenix.Session, title:string):phenix.Request
---@field models fun(self: phenix.Session):phenix.Request
---@field select_model fun(self: phenix.Session, model_id:string):phenix.Request
---@field routing_profiles fun(self: phenix.Session):phenix.Request
---@field select_routing_profile fun(self: phenix.Session, profile_id:string):phenix.Request
---@field provenance fun(self: phenix.Session, execution_id?:string):phenix.Request

---@class phenix.PermissionRequest
---@field session_id string
---@field execution_id string
---@field call_id string
---@field description string

---@class phenix.PermissionReply
---@field allow_once fun(self: phenix.PermissionReply)
---@field deny fun(self: phenix.PermissionReply)
---@field cancel fun(self: phenix.PermissionReply)

---@class phenix.FormField
---@field name string
---@field schema phenix.Form

---@class phenix.Form
---@field kind "string"|"boolean"|"integer"|"number"|"enum"|"list"|"object"
---@field optional? boolean
---@field signed? boolean
---@field options? string[]
---@field item? phenix.Form
---@field fields? phenix.FormField[]

---@class phenix.ElicitationRequest
---@field session_id string
---@field message string
---@field form phenix.Form

---@class phenix.ElicitationReply
---@field accept fun(self: phenix.ElicitationReply, value:any)
---@field decline fun(self: phenix.ElicitationReply)
---@field cancel fun(self: phenix.ElicitationReply)

---@class phenix.Event
---@field kind "status"|"session_snapshot"|"session_update"
---@field data table

---@class phenix.Features
---@field models boolean
---@field routing boolean
---@field provenance boolean
---@field review boolean

---@class phenix.Client
---@field status fun(self: phenix.Client):phenix.ClientStatus
---@field features fun(self: phenix.Client):phenix.Features
---@field sessions fun(self: phenix.Client):phenix.Sessions
---@field pump fun(self: phenix.Client, budget?:integer):phenix.Event[]
---@field decide_review fun(self: phenix.Client, review:table, decision:"accept"|"reject"):phenix.Request
---@field close fun(self: phenix.Client)
