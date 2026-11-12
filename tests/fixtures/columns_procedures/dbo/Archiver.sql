CREATE PROCEDURE dbo.Archiver
AS
    INSERT INTO dbo.Archive SELECT * FROM dbo.Clients;
GO
