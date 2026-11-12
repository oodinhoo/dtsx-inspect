CREATE PROCEDURE dbo.Ping
AS
    INSERT INTO dbo.PingLog (id) SELECT id FROM stg.Clients;
    EXEC dbo.Pong;
GO
