CREATE PROCEDURE dbo.Pong
AS
    INSERT INTO dbo.PongLog (id) SELECT id FROM dbo.PingLog;
    EXEC dbo.Ping;
GO
