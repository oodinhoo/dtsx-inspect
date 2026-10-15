CREATE PROCEDURE dbo.Doublon
AS
    INSERT INTO dbo.CibleB SELECT id FROM stg.Source;
GO
