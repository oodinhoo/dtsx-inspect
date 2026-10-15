CREATE PROCEDURE dbo.PurgerAdresse @client_id int
AS
    DELETE FROM dbo.Adresses WHERE client_id = @client_id;
GO
