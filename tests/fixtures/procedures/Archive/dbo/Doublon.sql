CREATE PROCEDURE dbo.Doublon
AS
    INSERT INTO dbo.CibleA SELECT id FROM stg.Source;
GO
